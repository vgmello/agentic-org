/**
 * The signed-in organizations page (FC-018 (1), (9)-(11)): what each listed organization
 * shows, and opening one. Opening always reads the list again, so an organization that
 * vanished or whose HQ went down after the list was shown is not opened from stale data.
 */
import type { ListReply } from './api';
import type { OfficeView, OrganizationList, OrganizationView } from './readers';

const STATUS: Record<OfficeView['status'], string> = {
	active: 'Online',
	starting: 'Starting',
	pending: 'Pending',
	offline: 'Offline',
	unavailable: 'Unavailable',
	update_needed: 'Update needed',
	unknown: 'Unknown'
};

export interface OrganizationRow {
	id: string;
	name: string;
	/** "HQ · hq-mac": the HQ office and the machine it runs on; undefined when it has no office. */
	office: string | undefined;
	status: string;
	/** The created date in the reader's locale; empty when the timestamp can't be read. */
	created: string;
	/** Its HQ can't serve it: listed, marked, and not opened. */
	unavailable: boolean;
	/** The organization this browser has selected. */
	openNow: boolean;
}

/** The first office is HQ (the reader keeps HQ first). */
const hq = (organization: OrganizationView): OfficeView | undefined => organization.offices[0];
export const isUnavailable = (organization: OrganizationView): boolean => {
	const office = hq(organization);
	return !office || office.status === 'unavailable';
};

export function createdDate(createdAt: string, locale?: string): string {
	const time = Date.parse(createdAt);
	if (Number.isNaN(time)) return '';
	return new Date(time).toLocaleDateString(locale, {
		year: 'numeric',
		month: 'short',
		day: 'numeric'
	});
}

export function organizationRows(
	organizations: readonly OrganizationView[],
	selected: string | undefined,
	locale?: string
): OrganizationRow[] {
	return organizations.map((organization) => {
		const office = hq(organization);
		return {
			id: organization.id,
			name: organization.name,
			office: office ? `${office.name} · ${office.host}` : undefined,
			status: office ? STATUS[office.status] : STATUS.unavailable,
			created: createdDate(organization.created_at, locale),
			unavailable: isUnavailable(organization),
			openNow: organization.id === selected
		};
	});
}

export type OpenResult =
	| { kind: 'opened'; organization: OrganizationView }
	/** It is no longer in the list: the fresh list is shown without it. */
	| { kind: 'vanished'; list: OrganizationList }
	/** Its HQ is unavailable now: the fresh list marks it. */
	| { kind: 'unavailable'; list: OrganizationList }
	| { kind: 'unreachable' }
	| { kind: 'session' };

/** Reads the list again and says whether `id` can be opened from it. It selects nothing. */
export async function openOrganization(
	api: { list(signal?: AbortSignal): Promise<ListReply> },
	id: string,
	signal?: AbortSignal
): Promise<OpenResult> {
	const reply = await api.list(signal);
	if (reply.kind !== 'listed') return reply;
	const organization = reply.list.organizations.find((o) => o.id === id);
	if (!organization) return { kind: 'vanished', list: reply.list };
	if (isUnavailable(organization)) return { kind: 'unavailable', list: reply.list };
	return { kind: 'opened', organization };
}
