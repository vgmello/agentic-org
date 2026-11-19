import { toAppError, type OfficeApi } from '$lib/office/api';
import type { AppError, Connection, Organization } from '$lib/proto';

/** What the welcome screen knows about the organizations on this Mac. */
export type Listing =
	| { kind: 'loading' }
	| { kind: 'ready'; organizations: Organization[]; current?: string }
	| { kind: 'failed'; error: AppError };

/**
 * Start the office if needed and list the organizations this Mac already has. `current` is the id
 * of the organization the app is connected to, if any. Never throws.
 */
export async function loadOrganizations(api: OfficeApi): Promise<Listing> {
	try {
		await api.ensureRunning();
		const organizations = await api.listOrganizations();
		// Which one is open is a nicety: if the remembered connections can't be read, still show the list.
		const current = await api
			.connections()
			.then((connections) => connections.at(-1)?.organization.id)
			.catch(() => undefined);
		return { kind: 'ready', organizations, current };
	} catch (e) {
		return { kind: 'failed', error: toAppError(e) };
	}
}

export type OpenResult = { ok: true; connection: Connection } | { ok: false; error: AppError };

/** Open one of the listed organizations. Never throws. */
export async function openOrganization(api: OfficeApi, id: string): Promise<OpenResult> {
	try {
		return { ok: true, connection: await api.openOrganization(id) };
	} catch (e) {
		return { ok: false, error: toAppError(e) };
	}
}

/** The office an organization is opened through on this Mac: its HQ, else its first office. */
export function localOffice(organization: Organization) {
	return organization.offices.find((o) => o.role === 'hq') ?? organization.offices[0];
}

/** Whether an organization can be opened: it has an office here that isn't unavailable. */
export function openable(organization: Organization): boolean {
	const office = localOffice(organization);
	return !!office && office.status !== 'unavailable';
}

/** "Sep 14, 2026" for an RFC 3339 timestamp; the raw text if it can't be read. */
export function formatDate(rfc3339: string): string {
	const date = new Date(rfc3339);
	if (Number.isNaN(date.getTime())) return rfc3339;
	return new Intl.DateTimeFormat('en', { dateStyle: 'medium', timeZone: 'UTC' }).format(date);
}
