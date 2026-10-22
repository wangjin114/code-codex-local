//! Prepare the official runtime without CopyFile's encrypted-source penalty.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, Write},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_GENERIC_READ, FILE_SHARE_READ,
};
use windows_sys::{
    Wdk::Storage::FileSystem::{
        FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
        RtlNtStatusToDosErrorNoTeb,
    },
    Win32::System::IO::IO_STATUS_BLOCK,
};

const CRITICAL: [&str; 3] = ["manifest.json", "bin/node.exe", "bin/node_repl.exe"];
const MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const LOCK_NAME: &str = ".code-codex-prewarm.lock";

fn commit_stage(stage: &mut tempfile::TempDir, handle: &File, key: &str) -> io::Result<()> {
    rename_locked(handle, key)?;
    stage.disable_cleanup(true);
    Ok(())
}

fn packaged_runtime_source(
    installation: &crate::discovery::CodexInstallation,
    unvirtualized: bool,
) -> io::Result<Option<PathBuf>> {
    if installation.source != crate::discovery::DiscoverySource::WindowsPackageManager {
        return Ok(None);
    }
    let source = installation
        .executable
        .parent()
        .ok_or_else(|| invalid("package executable has no parent"))?
        .join("resources/cua_node");
    match fs::symlink_metadata(&source) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    if !unvirtualized {
        return Err(invalid(
            "package runtime cache virtualization contract is unknown",
        ));
    }
    Ok(Some(source))
}

pub(crate) fn prepare_installation(
    installation: &crate::discovery::CodexInstallation,
) -> io::Result<Option<PreparationReport>> {
    let Some(_) = packaged_runtime_source(installation, installation.runtime_cache_unvirtualized)?
    else {
        return Ok(None);
    };
    // Discovery accepted the package location; resolve its trusted alias once,
    // before adding descendants, so internal junctions remain visible.
    let executable_parent = dunce::canonicalize(
        installation
            .executable
            .parent()
            .ok_or_else(|| invalid("package root missing"))?,
    )?;
    let physical_source = executable_parent.join("resources/cua_node");
    let _source_ancestry = lock_source_ancestry(&physical_source)?;
    validate_manifest(&physical_source)?;
    let known = known_folders::get_known_folder_path(known_folders::KnownFolder::LocalAppData)
        .ok_or_else(|| invalid("desktop LocalAppData is unavailable"))?;
    let inherited = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| invalid("desktop LOCALAPPDATA environment is unavailable"))?;
    let root = dunce::canonicalize(known)?;
    if !same_file::is_same_file(&root, inherited)? {
        return Err(invalid(
            "desktop and inherited LocalAppData contexts differ",
        ));
    }
    let mut handles = lock_ancestry(&root)?;
    let mut parent = root;
    for component in ["OpenAI", "Codex", "runtimes", "cua_node"] {
        parent.push(component);
        match fs::create_dir(&parent) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        handles.push(open_directory(&parent, false)?);
    }
    prepare_runtime(&physical_source, &parent).map(Some)
}

fn validate_manifest(source: &Path) -> io::Result<()> {
    let mut manifest = open_file(&source.join("manifest.json"))?;
    if manifest.metadata()?.len() > 1024 * 1024 {
        return Err(invalid("runtime manifest exceeds limit"));
    }
    let mut bytes = Vec::new();
    manifest.read_to_end(&mut bytes)?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| invalid("runtime manifest is invalid"))?;
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    };
    if value["platform"] != "windows"
        || value["arch"] != arch
        || value["node_path"] != "bin/node.exe"
        || value["node_repl_path"] != "bin/node_repl.exe"
        || value["node_modules"] != "bin/node_modules"
    {
        return Err(invalid("runtime manifest layout is unsupported"));
    }
    Ok(())
}

#[derive(Debug, Default)]
pub(crate) struct PreparationReport {
    pub(crate) prepared: bool,
    pub(crate) files: u64,
    pub(crate) bytes: u64,
}

pub(crate) fn prepare_runtime(source: &Path, parent: &Path) -> io::Result<PreparationReport> {
    prepare_with_budget(source, parent, MAX_BYTES)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

// Canonicalization is the caller's trust decision for root aliases. Do not
// canonicalize descendants here: doing so would hide junctions from validation.
fn lock_ancestry(path: &Path) -> io::Result<Vec<File>> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(invalid(
            "runtime path must be absolute without parent traversal",
        ));
    }
    let mut ancestors: Vec<_> = path.ancestors().filter(|p| p.is_absolute()).collect();
    ancestors.reverse();
    ancestors
        .into_iter()
        .map(|p| open_directory(p, false))
        .collect()
}

fn lock_source_ancestry(path: &Path) -> io::Result<Vec<File>> {
    // Production callers pass a physical package path already trusted by
    // discovery. WindowsApps itself denies directory-list access to ordinary
    // users; its protected ancestry is the existing package trust boundary.
    // Retain every component below that anchor, including the package root.
    let mut descendants: Vec<_> = path
        .ancestors()
        .take_while(|p| {
            !p.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("WindowsApps"))
        })
        .collect();
    if descendants.len() == path.ancestors().count() {
        return lock_ancestry(path);
    }
    descendants.reverse();
    descendants
        .into_iter()
        .map(|p| open_directory(p, false))
        .collect()
}

fn open_directory(path: &Path, rename: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    if rename {
        options.access_mode(FILE_GENERIC_READ | DELETE);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(invalid("runtime directory is not a plain directory"));
    }
    Ok(file)
}

fn open_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(invalid("runtime entry is not a plain file"));
    }
    Ok(file)
}

fn acquire_lock(parent: &Path, timeout: Duration) -> io::Result<File> {
    let started = Instant::now();
    loop {
        let result = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(parent.join(LOCK_NAME));
        match result {
            Ok(file) => {
                let metadata = file.metadata()?;
                if !metadata.is_file()
                    || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
                {
                    return Err(invalid("runtime preparation lock is not a plain file"));
                }
                return Ok(file);
            }
            Err(error)
                if matches!(error.raw_os_error(), Some(32 | 33)) && started.elapsed() < timeout =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Directory,
    File { size: u64, digest: [u8; 32] },
}
struct Tree {
    entries: BTreeMap<PathBuf, Entry>,
    files: BTreeMap<PathBuf, File>,
    directories: Vec<File>,
    bytes: u64,
}

fn read_tree(root: &Path) -> io::Result<Tree> {
    fn walk(root: &Path, relative: &Path, depth: usize, tree: &mut Tree) -> io::Result<()> {
        if depth > 64 {
            return Err(invalid("runtime directory depth exceeds limit"));
        }
        for entry in fs::read_dir(root.join(relative))? {
            let entry = entry?;
            if tree.entries.len() >= MAX_ENTRIES {
                return Err(invalid("runtime entry count exceeds limit"));
            }
            let child = relative.join(entry.file_name());
            let path = root.join(&child);
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(invalid("runtime contains a reparse point"));
            }
            if metadata.is_dir() {
                tree.directories.push(open_directory(&path, false)?);
                tree.entries.insert(child.clone(), Entry::Directory);
                walk(root, &child, depth + 1, tree)?;
            } else {
                let mut file = open_file(&path)?;
                let size = file.metadata()?.len();
                tree.bytes = tree
                    .bytes
                    .checked_add(size)
                    .filter(|n| *n <= MAX_BYTES)
                    .ok_or_else(|| invalid("runtime byte count exceeds limit"))?;
                let mut hasher = Sha256::new();
                let mut buffer = [0_u8; 64 * 1024];
                let mut read_total = 0_u64;
                loop {
                    let read = file.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    read_total += read as u64;
                    if read_total > size {
                        return Err(invalid("runtime source changed during hashing"));
                    }
                    hasher.update(&buffer[..read]);
                }
                if read_total != size {
                    return Err(invalid("runtime source changed during hashing"));
                }
                file.rewind()?;
                tree.entries.insert(
                    child.clone(),
                    Entry::File {
                        size,
                        digest: hasher.finalize().into(),
                    },
                );
                tree.files.insert(child, file);
            }
        }
        Ok(())
    }
    let mut tree = Tree {
        entries: BTreeMap::new(),
        files: BTreeMap::new(),
        directories: Vec::new(),
        bytes: 0,
    };
    walk(root, Path::new(""), 0, &mut tree)?;
    Ok(tree)
}

fn content_key(tree: &Tree) -> io::Result<String> {
    let mut hasher = Sha256::new();
    for name in CRITICAL {
        let Some(Entry::File { digest, .. }) = tree.entries.get(Path::new(name)) else {
            return Err(invalid("runtime is missing a critical file"));
        };
        hasher.update(name.as_bytes());
        hasher.update([0]);
        let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        hasher.update(hex.as_bytes());
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize())[..16].to_owned())
}

fn validate_target(target: &Path, source: &Tree) -> io::Result<()> {
    let _root = open_directory(target, false)?;
    let existing = read_tree(target)?;
    if source.entries != existing.entries {
        return Err(invalid(
            "existing runtime cache differs from trusted source; preserved",
        ));
    }
    Ok(())
}

fn prepare_with_budget(source: &Path, parent: &Path, budget: u64) -> io::Result<PreparationReport> {
    let _source_ancestry = lock_source_ancestry(source)?;
    let _parent_ancestry = lock_ancestry(parent)?;
    let _lock = acquire_lock(parent, Duration::from_secs(30))?;
    let mut source_tree = read_tree(source)?;
    if source_tree.entries.get(Path::new("bin/node_modules")) != Some(&Entry::Directory) {
        return Err(invalid("runtime node_modules directory is missing"));
    }
    let key = content_key(&source_tree)?;
    let target = parent.join(&key);
    let report = PreparationReport {
        prepared: false,
        files: source_tree.files.len() as u64,
        bytes: source_tree.bytes,
    };
    match fs::symlink_metadata(&target) {
        Ok(_) => {
            validate_target(&target, &source_tree)?;
            return Ok(report);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut stage = tempfile::Builder::new()
        .prefix(".code-codex-prewarm-")
        .tempdir_in(parent)?;
    let stage_handle = open_directory(stage.path(), true)?;
    let mut directories = Vec::new();
    let mut remaining = budget;
    for (relative, entry) in &source_tree.entries {
        let path = stage.path().join(relative);
        match entry {
            Entry::Directory => {
                fs::create_dir(&path)?;
                directories.push(open_directory(&path, false)?);
            }
            Entry::File { size, digest } => {
                let source = source_tree
                    .files
                    .get_mut(relative)
                    .ok_or_else(|| invalid("source file missing"))?;
                let mut destination = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .share_mode(FILE_SHARE_READ)
                    .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                    .open(&path)?;
                let mut hasher = Sha256::new();
                let mut buffer = [0_u8; 64 * 1024];
                let mut copied = 0;
                loop {
                    let count = source.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    remaining = remaining
                        .checked_sub(count as u64)
                        .ok_or_else(|| invalid("runtime copy byte budget exceeded"))?;
                    destination.write_all(&buffer[..count])?;
                    hasher.update(&buffer[..count]);
                    copied += count as u64;
                }
                let copied_digest: [u8; 32] = hasher.finalize().into();
                if copied != *size || copied_digest != *digest {
                    return Err(invalid("runtime copy source changed"));
                }
                destination.sync_all()?;
            }
        }
    }
    drop(directories);
    let copied = read_tree(stage.path())?;
    if source_tree.entries != copied.entries {
        return Err(invalid("runtime staging verification failed"));
    }
    drop(copied);
    match commit_stage(&mut stage, &stage_handle, &key) {
        Ok(()) => Ok(PreparationReport {
            prepared: true,
            ..report
        }),
        Err(error) => {
            if fs::symlink_metadata(&target).is_err() {
                return Err(error);
            }
            validate_target(&target, &source_tree)?;
            Ok(report)
        }
    }
}

// Rename using the retained DELETE-capable handle. No release/reopen window,
// no replacement flag, and all destination ancestors remain locked.
fn rename_locked(stage: &File, key: &str) -> io::Result<()> {
    if key.len() != 16 || !key.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid("invalid runtime content key"));
    }
    // A simple name with NULL RootDirectory renames within the stage's parent
    // without reopening it for write, which would conflict with our locks.
    let name: Vec<u16> = std::ffi::OsStr::new(key).encode_wide().collect();
    let offset = std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
    let size = offset + name.len() * 2;
    let mut storage = vec![0_usize; size.div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: usize storage provides pointer alignment; allocation covers the
    // variable-length UTF-16 payload. Windows reads it synchronously. The file
    // handle is valid and retained for the complete call.
    unsafe {
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        (*info).FileNameLength = (name.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
        let started = Instant::now();
        loop {
            let mut status_block = IO_STATUS_BLOCK::default();
            let status = NtSetInformationFile(
                stage.as_raw_handle(),
                &mut status_block,
                info.cast(),
                size as u32,
                FileRenameInformation,
            );
            if status >= 0 {
                break;
            }
            let code = RtlNtStatusToDosErrorNoTeb(status) as i32;
            if matches!(code, 5 | 32 | 33) && started.elapsed() < Duration::from_secs(1) {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            return Err(io::Error::from_raw_os_error(code));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const KEY: &str = "b5d5954590634650"; // Independently computed with Node SHA256.
    const FILES: [(&str, &[u8]); 4] = [
        ("manifest.json", b"manifest"),
        ("bin/node.exe", b"node"),
        ("bin/node_repl.exe", b"repl"),
        ("bin/node_modules/package/index.js", b"module.exports = 42;"),
    ];

    fn fixture() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let temp = tempfile::tempdir().expect("temp");
        let source = temp.path().join("source");
        let cache = temp.path().join("cache");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&cache).expect("cache");
        for (name, bytes) in FILES {
            let path = source.join(name);
            fs::create_dir_all(path.parent().expect("parent")).expect("parents");
            fs::write(path, bytes).expect("fixture bytes");
        }
        (temp, source, cache)
    }

    #[test]
    fn packaged_selection_requires_a_known_unvirtualized_context() {
        use crate::discovery::{CodexInstallation, DiscoverySource};
        let (temp, _source, _cache) = fixture();
        let runtime = temp.path().join("app/resources/cua_node");
        fs::create_dir_all(&runtime).expect("runtime");
        let mut installation = CodexInstallation {
            executable: temp.path().join("app/ChatGPT.exe"),
            version: "test".into(),
            channel: "test".into(),
            source: DiscoverySource::Explicit,
            package_name: None,
            runtime_cache_unvirtualized: false,
            package_full_name: None,
            app_user_model_id: None,
            app_server: None,
        };
        assert!(
            packaged_runtime_source(&installation, false)
                .expect("explicit no-op")
                .is_none()
        );
        installation.source = DiscoverySource::WindowsPackageManager;
        assert!(
            packaged_runtime_source(&installation, false).is_err(),
            "accepted virtualized cache context"
        );
        assert_eq!(
            packaged_runtime_source(&installation, true).expect("package"),
            Some(runtime.clone())
        );
        fs::remove_dir(&runtime).expect("remove optional runtime");
        assert!(
            packaged_runtime_source(&installation, false)
                .expect("missing no-op")
                .is_none()
        );
    }

    #[test]
    fn cold_cache_publishes_every_file_under_the_official_content_key() {
        let (_temp, source, cache) = fixture();
        let report = prepare_runtime(&source, &cache).expect("prepare");
        assert!(cache.join(KEY).is_dir(), "complete cache was not published");
        assert!(report.prepared);
        assert_eq!(report.files, 4);
        assert_eq!(report.bytes, 36);
        for (name, bytes) in FILES {
            assert_eq!(
                fs::read(cache.join(KEY).join(name)).expect("cached bytes"),
                bytes
            );
        }
    }

    #[test]
    fn warm_cache_reuses_without_rewriting() {
        let (_temp, source, cache) = fixture();
        prepare_runtime(&source, &cache).expect("cold");
        let file = cache.join(KEY).join("manifest.json");
        let modified = fs::metadata(&file)
            .expect("metadata")
            .modified()
            .expect("mtime");
        let report = prepare_runtime(&source, &cache).expect("warm");
        assert!(!report.prepared);
        assert_eq!(
            fs::metadata(file)
                .expect("metadata")
                .modified()
                .expect("mtime"),
            modified
        );
    }

    #[test]
    fn invalid_existing_tree_is_preserved_instead_of_overwritten() {
        let (_temp, source, cache) = fixture();
        let target = cache.join(KEY);
        fs::create_dir(&target).expect("existing");
        fs::write(target.join("sentinel"), b"keep").expect("sentinel");
        assert!(prepare_runtime(&source, &cache).is_err());
        assert_eq!(
            fs::read(target.join("sentinel")).expect("preserved"),
            b"keep"
        );
    }

    #[test]
    fn warm_cache_rejects_missing_extra_or_changed_module_bytes() {
        for mutation in ["missing", "extra", "changed", "source-changed"] {
            let (_temp, source, cache) = fixture();
            prepare_runtime(&source, &cache).expect("cold");
            let module = cache.join(KEY).join(FILES[3].0);
            match mutation {
                "missing" => fs::remove_file(&module).expect("remove"),
                "extra" => fs::write(cache.join(KEY).join("extra.js"), b"extra").expect("extra"),
                "changed" => fs::write(&module, b"changed").expect("change"),
                _ => fs::write(source.join(FILES[3].0), b"new source module").expect("source"),
            }
            assert!(
                prepare_runtime(&source, &cache).is_err(),
                "accepted {mutation}"
            );
        }
    }

    #[test]
    fn changed_critical_bytes_create_a_new_key_and_preserve_old_cache() {
        let (_temp, source, cache) = fixture();
        prepare_runtime(&source, &cache).expect("cold");
        fs::write(source.join("bin/node.exe"), b"different node").expect("change");
        assert!(
            prepare_runtime(&source, &cache)
                .expect("new runtime")
                .prepared
        );
        assert_eq!(
            fs::read(cache.join(KEY).join("bin/node.exe")).expect("old node"),
            b"node"
        );
        assert_eq!(
            fs::read_dir(cache)
                .expect("cache")
                .filter_map(Result::ok)
                .filter(|e| e.path().is_dir())
                .count(),
            2
        );
    }

    #[test]
    fn source_and_destination_junctions_leave_external_sentinels_unchanged() {
        for destination in [false, true] {
            let (temp, source, cache) = fixture();
            let external = temp.path().join("external");
            fs::create_dir(&external).expect("external");
            fs::write(external.join("sentinel"), b"keep").expect("sentinel");
            let path = if destination {
                cache.join(KEY)
            } else {
                source.join("escape")
            };
            junction::create(&external, &path).expect("junction");
            assert!(
                prepare_runtime(&source, &cache).is_err(),
                "accepted junction"
            );
            assert_eq!(
                fs::read(external.join("sentinel")).expect("sentinel"),
                b"keep"
            );
            junction::delete(path).expect("unlink junction");
        }
    }

    #[test]
    fn concurrent_preparations_converge_on_one_complete_cache() {
        let (_temp, source, cache) = fixture();
        let (a, b) = std::thread::scope(|s| {
            let a = s.spawn(|| prepare_runtime(&source, &cache));
            let b = s.spawn(|| prepare_runtime(&source, &cache));
            (
                a.join().expect("thread").expect("first"),
                b.join().expect("thread").expect("second"),
            )
        });
        assert_ne!(a.prepared, b.prepared);
        assert_eq!(
            fs::read(cache.join(KEY).join(FILES[3].0)).expect("complete"),
            FILES[3].1
        );
    }

    #[test]
    fn interrupted_copy_removes_only_its_unpublished_stage() {
        let (_temp, source, cache) = fixture();
        fs::create_dir(cache.join(".staging-native-untouched")).expect("native stage");
        assert!(prepare_with_budget(&source, &cache, 5).is_err());
        assert!(!cache.join(KEY).exists());
        let names: Vec<_> = fs::read_dir(&cache)
            .expect("cache")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(names.len(), 2, "left unpublished private stage");
        assert!(cache.join(".staging-native-untouched").is_dir());
    }

    #[test]
    fn exclusive_lock_times_out_then_recovers_when_handle_is_dropped() {
        let (_temp, _source, cache) = fixture();
        let lock = acquire_lock(&cache, Duration::ZERO).expect("first lock");
        assert!(acquire_lock(&cache, Duration::from_millis(30)).is_err());
        drop(lock); // Process exit releases the same kernel handle lock.
        let _recovered = acquire_lock(&cache, Duration::ZERO).expect("released lock");
    }

    #[test]
    fn retained_directory_and_file_handles_block_checked_path_replacement() {
        let (_temp, source, _cache) = fixture();
        let parent = source.join("bin/node_modules");
        let _directory = open_directory(&parent, false).expect("checked directory");
        assert!(fs::rename(&parent, source.join("swapped-parent")).is_err());
        let leaf = source.join("bin/node.exe");
        let _file = open_file(&leaf).expect("checked file");
        assert!(fs::rename(&leaf, source.join("swapped-file")).is_err());
        assert!(fs::write(&leaf, b"replace").is_err());
        assert_eq!(fs::read(&leaf).expect("intact"), b"node");
    }

    #[test]
    fn handle_commit_never_replaces_a_target_created_by_another_writer() {
        let (_temp, _source, cache) = fixture();
        let stage = tempfile::Builder::new()
            .prefix(".code-codex-prewarm-")
            .tempdir_in(&cache)
            .expect("stage");
        let handle = open_directory(stage.path(), true).expect("stage handle");
        let target = cache.join(KEY);
        fs::create_dir(&target).expect("competing target");
        fs::write(target.join("sentinel"), b"keep").expect("sentinel");
        assert!(rename_locked(&handle, KEY).is_err());
        assert_eq!(
            fs::read(target.join("sentinel")).expect("preserved"),
            b"keep"
        );
        drop(handle);
    }

    #[test]
    fn retained_handle_commits_a_real_directory() {
        let (_temp, _source, cache) = fixture();
        let _ancestry = lock_ancestry(&cache).expect("parent locks");
        let stage = tempfile::Builder::new()
            .prefix(".code-codex-prewarm-")
            .tempdir_in(&cache)
            .expect("stage");
        fs::write(stage.path().join("data"), b"data").expect("file");
        let handle = open_directory(stage.path(), true).expect("handle");
        rename_locked(&handle, KEY).expect("handle rename");
        assert_eq!(
            fs::read(cache.join(KEY).join("data")).expect("published"),
            b"data"
        );
        drop(handle);
    }

    #[test]
    fn windows_directory_commit_requires_child_data_handles_to_be_closed() {
        let (_temp, source, _cache) = fixture();
        let handle = open_directory(&source, true).expect("root");
        let verified = read_tree(&source).expect("verified children");
        assert!(rename_locked(&handle, KEY).is_err());
        drop(verified);
        drop(handle);
    }

    #[test]
    fn directory_commit_survives_a_transient_child_handle() {
        let (_temp, source, _cache) = fixture();
        let handle = open_directory(&source, true).expect("root");
        let verified = read_tree(&source).expect("temporary reader");
        let reader = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(verified);
        });
        let result = rename_locked(&handle, KEY);
        reader.join().expect("reader");
        result.expect("commit after transient reader releases handles");
        drop(handle);
    }

    #[test]
    fn successful_commit_does_not_clean_a_reused_old_stage_name() {
        let (_temp, _source, cache) = fixture();
        let mut stage = tempfile::Builder::new()
            .prefix(".code-codex-prewarm-")
            .tempdir_in(&cache)
            .expect("stage");
        let old = stage.path().to_path_buf();
        let handle = open_directory(stage.path(), true).expect("root");
        commit_stage(&mut stage, &handle, KEY).expect("publish");
        fs::create_dir(&old).expect("another writer reuses old name");
        fs::write(old.join("sentinel"), b"keep").expect("foreign data");
        drop(handle);
        drop(stage);
        assert_eq!(
            fs::read(old.join("sentinel")).expect("foreign directory must survive"),
            b"keep"
        );
    }

    #[test]
    fn unknown_manifest_layout_fails_before_cache_creation() {
        let (_temp, source, _cache) = fixture();
        fs::write(
            source.join("manifest.json"),
            br#"{"platform":"windows","arch":"x64","node_path":"other.exe"}"#,
        )
        .expect("manifest");
        assert!(validate_manifest(&source).is_err());
    }

    #[test]
    #[ignore = "requires an explicitly selected real encrypted package source"]
    fn encrypted_package_source_is_prepared_with_identical_bytes() {
        let source = PathBuf::from(
            std::env::var_os("CODE_CODEX_TEST_RUNTIME_SOURCE").expect("explicit source"),
        );
        let destination = PathBuf::from(
            std::env::var_os("CODE_CODEX_TEST_RUNTIME_DESTINATION").expect("explicit destination"),
        );
        assert_ne!(
            fs::metadata(source.join("manifest.json"))
                .expect("source metadata")
                .file_attributes()
                & 0x4000,
            0,
            "source must reproduce encrypted package files"
        );
        let started = Instant::now();
        let report = prepare_runtime(&source, &destination).expect("EFS byte preparation");
        assert!(report.files > 2000);
        eprintln!(
            "real EFS preparation: files={} bytes={} prepared={} elapsedMs={}",
            report.files,
            report.bytes,
            report.prepared,
            started.elapsed().as_millis()
        );
    }
}
