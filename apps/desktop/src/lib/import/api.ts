import { inTauri } from '$lib/office/api';
import { mockImport } from './mock';
import { tauriImport } from './tauri';
import type { Defaults, Scan } from './types';

/** What "Bring in an existing team" needs from the shell: the usual places, a folder picker, and a read-only scan. */
export interface ImportApi {
	/** The usual places an existing team lives in. */
	defaults(): Promise<Defaults>;
	/** Read the HR workspace and the messaging spool. A source not given is not read; one that can't be read says why. */
	scan(hrDir: string | undefined, spoolDir: string | undefined): Promise<Scan>;
	/** Ask for a folder with the system's picker. `undefined`: cancelled. */
	pickFolder(title: string): Promise<string | undefined>;
}

export const importApi: ImportApi = inTauri() ? tauriImport : mockImport;
