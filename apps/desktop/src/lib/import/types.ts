// What the shell's read-only scan finds (`import_scan`, src-tauri/src/import_scan.rs). Missing stays missing.

export interface HrAgent {
	id: string;
	name?: string;
	project?: string;
	role?: string;
	title?: string;
	department?: string;
	focus?: string;
	/** An HR id, or `user` for the owner. */
	reports_to?: string;
	model?: string;
	/** `active`, `offboarded`, ... as HR wrote it. */
	status?: string;
	hired_on?: string;
	/** As HR wrote it, relative to the HR workspace. */
	home?: string;
	/** Where it is on this Mac (absent when HR's path would leave the workspace). */
	home_path?: string;
}

export interface HrScan {
	path: string;
	/** Why the workspace could not be read at all. */
	error?: string;
	agents: HrAgent[];
	roles: string[];
	/** Records skipped, each with why. */
	skipped: string[];
}

export interface MessagingScan {
	path: string;
	error?: string;
	names: string[];
}

export interface Scan {
	hr?: HrScan;
	messaging?: MessagingScan;
}

/** The usual places, found or not. */
export interface Defaults {
	hr?: string;
	messaging?: string;
	/** The name this Mac's user goes by (the owner's own inbox is not an agent). */
	user?: string;
}
