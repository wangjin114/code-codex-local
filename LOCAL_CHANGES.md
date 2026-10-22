# Local maintenance snapshot: 0.5.7-local.1

This is an unofficial private backup of the 0.5.7 build installed and verified on 2026-10-19, uploaded on 2026-10-22. Original author/project: Rice-dog and Code-Codex contributors, https://github.com/Rice-dog/code-codex. Immutable upstream commit: dbbc665b677624a26b98fdcd41f1db3f1a407bd2. The application version remains 0.5.7; the separate GitHub release tag is v0.5.7-local.1 to distinguish this maintenance build.

## Retained local changes

- Prepare and verify the official Windows packaged runtime cache before activation and before the CDP endpoint wait. Retain ownership, path, file identity and generation checks.
- Copy legacy EFS-protected data by reading bytes, verifying SHA-256, closing handles and committing only the owned stage; preserve originals and newer destinations.
- Retain per-document bounded CDP ingress and startup-burst regression coverage; preserve upstream lifecycle and localization architecture.
- Preserve trim/natural-end video completion, final-frame hold, readiness/minimum-visible gating and compositor fade in startup player 1.1.26-local.1.
- Recover committed keyboard file-operation state after partial failure, refresh affected directories, preserve session isolation and always release the mutation lease.
- Keep browser fixture discovery compatible with installed Chrome or Edge and retain Win10 UI-only timing bounds around both ownership checks.
- Package the exact verified local startup player as a sidecar with presence, staging, final-hash and retention gates. The other 24 public plugin descriptors and resource bytes match upstream.

## Source and binary correspondence

Source product files and the five local-player artifact files were copied byte-for-byte from the maintenance workspace. Only publication documentation and the publication gitignore are added or changed. The original local checkout, its history and personal maintenance records are not included. SOURCE_MANIFEST.json records the product source and local-player inputs; BUILD_PROVENANCE.json records artifact hashes and the tested boundary.

Release installer and ZIP bytes are the exact source-built artifacts previously used for installation, not a new compilation or upstream prebuilt downloads. Current installed native/player file hashes match that build. This establishes deployed binary identity, not a claim of byte-reproducible native builds from this fresh snapshot. Inherited package README download links and SBOM namespace are unchanged; use this repository's release metadata to identify this local build.

Build with Node24, npm ci in packages/explorer-ui, npm run build there, and scripts/build.ps1 -Configuration Release -SkipUi. The build script supplies static CRT. Published plugin assets and the exact local sidecar closure are included so the distribution lock can be verified offline once dependencies are available. Do not bypass lock input verification.

## Verification and limitations

- UI build/typecheck and 20 UI/integration domains passed after two fixture repairs; local player completion and original-media behavior were exercised.
- 588 unique native tests passed across partitioned runs, with six conditional ignores. Two unchanged 2.5-second CIM recovery checks time out during continuous full-suite runs; all eight recovery tests pass in the isolated actual-user run. The continuous suite is not unqualified green.
- EFS migration7, storage27, retention18, plugin contracts22 and versioned launcher18 checks passed. Independent source review accepted with zero remaining Critical/Important/Minor findings.
- Two real launches after installation qualified with fresh processes/logs, responsive UI, runtime preflight before activation and cache reuse. Existing settings and media matched; no OS reboot, MSI installation, EXE GUI execution or restore drill is claimed.
- At the installation-date audit there were two moderate dependency advisories. A fresh 2026-10-22 audit of the same locked dependencies reports one high-severity package (markdown-it) and one moderate-severity package (DOMPurify); see DEPENDENCY_AUDIT.json. Dependencies are unchanged to preserve this exact installed version. Markdown-it linkify is used; DOMPurify IN_PLACE is not used by these consumers. No security-clean claim is made.
- Keep THIRD_PARTY_NOTICES_EN.md and THIRD_PARTY_NOTICES_ZH_CN.md: some visual effects have conditional or unverified redistribution evidence. This repository is private personal backup, not a declaration that all assets are MIT or cleared for public release.

No chat history, account settings, user media, application profile, private backup, credentials, local source-control metadata or raw diagnostic logs are included.

Independent publication-snapshot UI rebuild reproduced explorer.js and startup-early.js byte-for-byte. Native binary reproducibility is still not claimed.
