import type { AppError, Member, Team } from '$lib/proto';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { readJsonObject, readResponse } from '$lib/platform/http/wire';
import { readOrganizationsList, type OrganizationView } from '$lib/platform/organizations/readers';
import type { OrgApi } from '$lib/modules/organization/api';
import type { GatewayAccessEntry } from '$lib/modules/members/http-api';
import type { AccessApi } from './api';
import type { AccessAdministrator, AccessOverview, AccessPerson, AccessState } from './types';

export interface AccessSessionInfo {
	accountAdmin: boolean;
	login: string;
}

function personState(active: boolean, entry: GatewayAccessEntry | undefined): AccessState {
	if (!active) return 'no_account';
	if (entry?.account_id) return 'signed_in';
	if (entry?.pending_invite) return 'invited';
	return 'no_account';
}

function mapAccessPerson(
	m: Member,
	teamsById: ReadonlyMap<string, Team>,
	entry: GatewayAccessEntry | undefined
): AccessPerson {
	const active = true;
	const state = personState(active, entry);
	const homeTeam = m.home_team_id ? teamsById.get(m.home_team_id)?.name : undefined;
	return {
		memberId: m.id,
		name: m.name,
		handle: m.handle ?? m.name,
		...(homeTeam ? { homeTeam } : {}),
		owner: Boolean(m.owner),
		administrator: Boolean(entry?.admin),
		active,
		state,
		...(state === 'signed_in' && entry?.login ? { username: entry.login } : {})
	};
}

function buildAdministrator(
	people: AccessPerson[],
	session: AccessSessionInfo | undefined
): AccessAdministrator {
	const adminPerson = people.find((p) => p.administrator);
	if (adminPerson) {
		return {
			...(adminPerson.username ? { username: adminPerson.username } : {}),
			memberId: adminPerson.memberId,
			name: adminPerson.name,
			isYou: Boolean(session?.accountAdmin)
		};
	}
	return {
		...(session?.accountAdmin ? { username: session.login } : {}),
		isYou: Boolean(session?.accountAdmin)
	};
}

/**
 * Production HTTP adapter for Organization settings › Members and access
 * (`/api/v1/organizations/{org}/access` + snapshot). Never falls back to mocks.
 */
export function createHttpAccessApi({
	http,
	orgApi,
	session
}: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	session: () => AccessSessionInfo | undefined;
}): AccessApi {
	async function readEntries(
		organizationId: string
	): Promise<{ entries: GatewayAccessEntry[]; canManage: boolean }> {
		let reply: GatewayReply;
		try {
			reply = await http.read(`/api/v1/organizations/${encodeURIComponent(organizationId)}/access`);
		} catch {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'Access settings could not be reached.'
			} satisfies AppError;
		}
		if (reply.status === 403) return { entries: [], canManage: false };
		const result = unwrapGatewayResult(reply, 'Access settings could not be loaded.');
		const entries = Array.isArray(result.entries)
			? (result.entries.filter(
					(item): item is GatewayAccessEntry =>
						Boolean(item) &&
						typeof item === 'object' &&
						typeof (item as { member_id?: unknown }).member_id === 'string'
				) as GatewayAccessEntry[])
			: [];
		return { entries, canManage: true };
	}

	async function readOrgView(organizationId: string): Promise<OrganizationView | undefined> {
		try {
			const reply = await http.read('/api/v1/organizations');
			if (reply.status !== 200) return undefined;
			const envelope = readResponse(reply.body, readJsonObject);
			if ('error' in envelope) return undefined;
			return readOrganizationsList(envelope.result).organizations.find(
				(o) => o.id === organizationId
			);
		} catch {
			return undefined;
		}
	}

	return Object.freeze({
		async overview(organizationId: string): Promise<AccessOverview> {
			const [{ snapshot }, accessData, org] = await Promise.all([
				orgApi.view(organizationId),
				readEntries(organizationId),
				readOrgView(organizationId)
			]);
			const teamsById = new Map(snapshot.teams.map((t) => [t.id, t]));
			const entriesById = new Map(accessData.entries.map((e) => [e.member_id, e]));
			const people = snapshot.members
				.filter((m) => m.kind === 'person')
				.map((m) => mapAccessPerson(m, teamsById, entriesById.get(m.id)));
			const current = session();
			const viewerPerson = org?.employee_id
				? people.find((p) => p.memberId === org.employee_id)
				: undefined;
			const isOwner = Boolean(viewerPerson?.owner);
			const isAdministrator = Boolean(current?.accountAdmin);
			return {
				organization: { id: organizationId, name: org?.name ?? organizationId },
				administrator: buildAdministrator(people, current),
				people,
				viewer: {
					...(org?.employee_id ? { memberId: org.employee_id } : {}),
					isOwner,
					isAdministrator,
					canManage: accessData.canManage
				}
			};
		}
	});
}
