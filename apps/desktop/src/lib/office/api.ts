import {
	API_ERROR_CODES,
	type ApiErrorCode,
	type AppError,
	type Connection,
	type HelloResult,
	type Organization
} from '$lib/proto';
import { mockOffice } from './mock';
import { tauriOffice } from './tauri';

/**
 * What the UI needs from an office. Two implementations: Tauri (the Rust shell, which owns the
 * connection) and a mock that runs in a plain browser, for `bun run dev` and tests.
 */
export interface OfficeApi {
	/** Offices and organizations remembered from earlier launches. */
	connections(): Promise<Connection[]>;
	/** Make sure an office is installed, running and answering. */
	ensureRunning(): Promise<HelloResult>;
	/** Whether the running office answers. Read-only (never starts anything): safe to call often. */
	ping(): Promise<void>;
	/** Create an organization on this Mac's host (reached by `ensureRunning`), and remember it. */
	createOrganization(name: string): Promise<Connection>;
	/** The organizations this Mac already has, each with its offices here (`organization.list`). */
	listOrganizations(): Promise<Organization[]>;
	/** Open one of them: remember the connection to its office here. */
	openOrganization(id: string): Promise<Connection>;
}

/** Normalize anything thrown by a call into an `AppError`. */
export function toAppError(e: unknown): AppError {
	if (e && typeof e === 'object' && 'source' in e && 'code' in e && 'message' in e) {
		const error = e as AppError;
		// A newer office may send a code this build doesn't know: treat it as `unknown`.
		if (error.source === 'api' && !API_ERROR_CODES.includes(error.code as ApiErrorCode))
			return { ...error, code: 'unknown' };
		return error;
	}
	return { source: 'app', code: 'internal', message: e instanceof Error ? e.message : String(e) };
}

export function inTauri(): boolean {
	return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export const officeApi: OfficeApi = inTauri() ? tauriOffice : mockOffice;
