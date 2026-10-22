import {existsSync} from 'node:fs';
import {join} from 'node:path';

// Use an isolated headless profile; never reuse the user's browser session.
export function browserExecutable() {
  const override = process.env.CODE_CODEX_TEST_BROWSER;
  if (override) {
    if (!existsSync(override)) throw new Error(`Test browser does not exist: ${override}`);
    return override;
  }
  const roots = [process.env.PROGRAMFILES, process.env['PROGRAMFILES(X86)'], process.env.LOCALAPPDATA,
    'C:/Program Files', 'C:/Program Files (x86)'].filter(Boolean);
  for (const root of roots) {
    for (const relative of ['Google/Chrome/Application/chrome.exe', 'Microsoft/Edge/Application/msedge.exe']) {
      const candidate = join(root, relative);
      if (existsSync(candidate)) return candidate;
    }
  }
  throw new Error('Chrome or Edge was not found. Set CODE_CODEX_TEST_BROWSER to a Chromium browser executable.');
}
