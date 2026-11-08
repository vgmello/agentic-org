// Hand-written mirror of the `org-proto` crate's types (crates/org-proto/src/{types,calls,error}.rs).
// Keep every wire type in this one file, so swapping in generated types touches nothing else.

/** An office's role in its organization. Missing or unrecognised means `unknown`, never HQ. */
export type OfficeRole = 'hq' | 'satellite' | 'unknown';

/** Missing or unrecognised means `unknown`, never healthy. */
export type OfficeStatus =
	| 'active'
	| 'starting'
	| 'pending'
	| 'offline'
	| 'unavailable'
	| 'update_needed'
	| 'unknown';

/** An office: a node of one organization, run by its own server process. */
export interface OfficeInfo {
	id: string;
	organization_id: string;
	name: string;
	role?: OfficeRole;
	/** Where it runs, for display: "This Mac" or a machine name. */
	host: string;
	/** How to reach it from this machine, when it is on this machine. */
	endpoint?: string;
	status?: OfficeStatus;
}

/** Set when an office server answers `hello`. */
export interface OfficeHello {
	organization_name: string;
	office: OfficeInfo;
}

export interface HelloResult {
	api_version: number;
	server_version: string;
	/** The machine, for display: "This Mac". */
	host: string;
	/** Absent when this machine's host answers. */
	office?: OfficeHello;
}

export interface Organization {
	id: string;
	name: string;
	/** RFC 3339, UTC, with milliseconds. Ids are opaque (ULIDs): never parse them. */
	created_at: string;
	/** Its offices; from this machine's host, its offices on this machine (HQ first). */
	offices: OfficeInfo[];
}

/**
 * An organization and its office on this machine (HQ first), which the app is connected to.
 * App-local: remembered between launches, not part of `org-proto`.
 */
export interface Connection {
	office: OfficeInfo;
	organization: Organization;
}

export interface OrganizationCreateParams {
	name: string;
}

export interface ListOrganizationsResult {
	organizations: Organization[];
}

/**
 * Errors the office reports (`org-proto`). Treat any code you don't recognise as `unknown`:
 * the server may grow new ones.
 */
export const API_ERROR_CODES = [
	'invalid_request',
	'unknown_method',
	'invalid_name',
	'already_exists',
	'storage_error',
	'unknown'
] as const;
export type ApiErrorCode = (typeof API_ERROR_CODES)[number];

/** Errors from making the office ready. These belong to the app, not to `org-proto`. */
export type SetupErrorCode = 'not_installed' | 'needs_approval' | 'unreachable';

/** What every failed call looks like to the UI. `source` says which layer failed. */
export type AppError =
	| { source: 'api'; code: ApiErrorCode; message: string }
	| { source: 'setup'; code: SetupErrorCode; message: string }
	| { source: 'app'; code: string; message: string };

/** Trimmed, no control characters. The office is the authority; the app only pre-checks. */
export const ORGANIZATION_NAME_MAX_CHARS = 64;
