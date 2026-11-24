import type { OrgApi } from '$lib/modules/organization/api';
import type { HomeApi } from './api';
import type { HomeProject, MonthSummary, WorkingNow } from './types';

export interface HttpHomeApiOptions {
	orgApi: Pick<OrgApi, 'view'>;
	now?: () => string;
}

/**
 * Production HTTP adapter for Home backed by live organization snapshots (`orgApi.view`).
 * Returns `month: null` cleanly until L2 cost reporting lands; never fabricates counts or mocks.
 */
export function createHttpHomeApi({
	orgApi,
	now = () => new Date().toISOString()
}: HttpHomeApiOptions): HomeApi {
	return Object.freeze({
		async projects(organizationId: string): Promise<HomeProject[]> {
			const { snapshot } = await orgApi.view(organizationId);
			const projectTeams = snapshot.project_teams ?? [];
			return (snapshot.projects ?? [])
				.filter((p) => !p.archived)
				.map((p): HomeProject => {
					const teamIds = new Set(
						projectTeams.filter((link) => link.project_id === p.id).map((link) => link.team_id)
					);
					const memberIds = new Set(
						snapshot.members
							.filter(
								(m) =>
									!m.owner &&
									m.agent?.status !== 'offboarded' &&
									m.home_team_id &&
									teamIds.has(m.home_team_id)
							)
							.map((m) => m.id)
					);
					return {
						id: p.id,
						name: p.name,
						done: 0,
						total: 0,
						open: 0,
						blocked: 0,
						members: memberIds.size
					};
				})
				.sort((a, b) => a.name.localeCompare(b.name));
		},

		async workingNow(organizationId: string): Promise<WorkingNow> {
			const { snapshot } = await orgApi.view(organizationId);
			const fallbackSince = now();
			const agents = snapshot.members
				.filter((m) => m.kind === 'agent' && m.agent?.status === 'active')
				.map((m) => ({
					member: { id: m.id, name: m.name },
					since: m.created_at ?? fallbackSince
				}));
			return { live: true, agents };
		},

		async month(): Promise<MonthSummary | null> {
			return null;
		}
	});
}
