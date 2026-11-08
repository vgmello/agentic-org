// Hand-written mirror of the `org-proto` crate's types (crates/org-proto/src/{types,calls,error}.rs).
// Keep every wire type in this one file, so swapping in generated types touches nothing else.

export interface OfficeInfo {
	id: string;
	name: string;
	host: string;
}

export interface HelloResult {
	api_version: number;
	server_version: string;
	office: OfficeInfo;
}

export interface Organization {
	id: string;
	name: string;
	/** RFC 3339, UTC, with milliseconds. Ids are opaque (ULIDs): never parse them. */
	created_at: string;
}

/** An office and the organization this app is connected to on it. Remembered between launches. */
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
