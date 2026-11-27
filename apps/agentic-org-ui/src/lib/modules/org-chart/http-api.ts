import type { Member, Team } from '$lib/proto';
import type { GatewayHttpTransport } from '$lib/platform/http/gateway-errors';
import { readJsonObject, readResponse } from '$lib/platform/http/wire';
import { readOrganizationsList, type OrganizationView } from '$lib/platform/organizations/readers';
import type { OrgApi } from '$lib/modules/organization/api';
import type { MembersApi, TeamRef } from '$lib/modules/members';
import type { OrgChartApi } from './api';
import type { ChartEmployee, OrgChart } from './types';

function teamRef(team: Team | undefined): TeamRef | undefined {
	if (!team) return undefined;
	return { id: team.id, name: team.name, ref: '' };
}

function memberHandle(m: Member): string {
	return m.handle ?? (m.kind === 'agent' ? (m.agent?.messaging_name ?? m.id) : m.name);
}

/**
 * Production HTTP adapter for the Org chart (`/teams/org-chart`) backed by live org snapshot,
 * members directory (for sign-in status) and `members/set-reports-to`. Never falls back to mocks.
 */
export function createHttpOrgChartApi({
	http,
	orgApi,
	membersApi
}: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view' | 'command'>;
	membersApi: Pick<MembersApi, 'directory'>;
}): OrgChartApi {
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
		async chart(organizationId: string): Promise<OrgChart> {
			const [{ snapshot }, rows, org] = await Promise.all([
				orgApi.view(organizationId),
				membersApi.directory(organizationId),
				readOrgView(organizationId)
			]);
			const teamsById = new Map(snapshot.teams.map((t) => [t.id, t]));
			const rowsById = new Map(rows.map((r) => [r.id, r]));
			const active = snapshot.members.filter((m) => m.agent?.status !== 'offboarded');
			const owner = active.find((m) => m.owner);

			const employees: ChartEmployee[] = active.map((m) => {
				const row = rowsById.get(m.id);
				const leads = snapshot.teams
					.filter((t) => t.lead_member_id === m.id)
					.map((t) => teamRef(t)!);
				const homeTeam = m.home_team_id ? teamRef(teamsById.get(m.home_team_id)) : undefined;
				return {
					id: m.id,
					kind: m.kind === 'agent' ? 'agent' : 'person',
					name: m.name,
					handle: row?.handle ?? memberHandle(m),
					...(m.builtin_key ? { builtinKey: m.builtin_key } : {}),
					status: m.kind === 'agent' && m.agent?.status === 'paused' ? 'paused' : 'active',
					...(homeTeam ? { homeTeam } : {}),
					...(m.reports_to_member_id ? { reportsToId: m.reports_to_member_id } : {}),
					leads,
					...(m.kind === 'agent' ? { office: row?.office ?? null } : {}),
					canSignIn: row?.canSignIn ?? false,
					revision: m.version
				};
			});

			return {
				organization: { id: organizationId, name: org?.name ?? organizationId },
				ownerId: owner?.id ?? '',
				...(org?.employee_id ? { viewerId: org.employee_id } : {}),
				employees
			};
		},

		async changeManager(
			organizationId: string,
			employeeId: string,
			managerId: string | null,
			expectedRevision: number
		): Promise<void> {
			await orgApi.command(organizationId, {
				method: 'member.set_reports_to',
				params: {
					member_id: employeeId,
					expected_version: expectedRevision,
					...(managerId ? { reports_to_member_id: managerId } : {})
				}
			});
		}
	});
}
