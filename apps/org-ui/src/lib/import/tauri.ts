import { invoke } from '@tauri-apps/api/core';
import type { ImportApi } from './api';

/** The scan runs in the Rust shell: the WebView reads no files. */
export const tauriImport: ImportApi = {
	defaults: () => invoke('import_defaults'),
	scan: (hrDir, spoolDir) => invoke('import_scan', { hrDir, spoolDir }),
	readHints: (path) => invoke('import_read_hints', { path }),
	pickFile: (title) => invoke<string | null>('import_pick_file', { title }).then((path) => path ?? undefined),
	pickFolder: (title) => invoke<string | null>('import_pick_folder', { title }).then((path) => path ?? undefined)
};
