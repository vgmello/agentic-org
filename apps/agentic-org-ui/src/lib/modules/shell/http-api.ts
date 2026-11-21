import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import type { OrgApi } from '$lib/modules/organization/api';
import type { ShellApi } from './api';
import type { NavPlace, SearchHit, SearchResults, ShellNavigation } from './types';

interface GatewayProjectNav {
	id: string;
	key?: string;
	name: string;
	slug?: string;
	archived?: boolean;
}

function isProjectNav(value: unknown): value is GatewayProjectNav {
	if (!value || typeof value !== 'object') return false;
	const p = value as Partial<GatewayProjectNav>;
	return typeof p.id === 'string' && typeof p.name === 'string';
}

const MAX_HITS = 5;

/**
 * Production HTTP adapter for app-shell navigation lists (live projects & teams) and search.
 * Never falls back to browser mocks.
 */
export function createHttpShellApi({
	http,
	orgApi
}: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
}): ShellApi {
	async function readProjects(organizationId: string): Promise<GatewayProjectNav[]> {
		let reply: GatewayReply;
		try {
			reply = await http.read(
				`/api/v1/organizations/${encodeURIComponent(organizationId)}/projects?include_archived=false`
			);
		} catch {
			return [];
		}
		if (reply.status !== 200) return [];
		const result = unwrapGatewayResult(reply, 'Projects could not be loaded.');
		return Array.isArray(result.projects) ? result.projects.filter(isProjectNav) : [];
	}

	return Object.freeze({
		async navigation(organizationId: string): Promise<ShellNavigation> {
			const [{ snapshot }, projects] = await Promise.all([
				orgApi.view(organizationId),
				readProjects(organizationId)
			]);
			const teamPlaces: NavPlace[] = snapshot.teams.map((t) => ({ id: t.id, name: t.name }));
			const projectPlaces: NavPlace[] = projects
				.filter((p) => !p.archived)
				.map((p) => ({ id: p.id, name: p.name }));
			return {
				inboxOpen: null,
				projects: projectPlaces,
				teams: teamPlaces
			};
		},

		async search(organizationId: string, query: string): Promise<SearchResults> {
			const needle = query.trim().toLowerCase();
			if (!needle) return { projects: [], work: [], members: [] };
			const [{ snapshot }, projects] = await Promise.all([
				orgApi.view(organizationId),
				readProjects(organizationId)
			]);
			const projectHits: SearchHit[] = projects
				.filter(
					(p) =>
						!p.archived &&
						(p.name.toLowerCase().includes(needle) ||
							(p.key && p.key.toLowerCase().includes(needle)) ||
							(p.slug && p.slug.toLowerCase().includes(needle)))
				)
				.slice(0, MAX_HITS)
				.map((p) => ({
					id: p.id,
					title: p.name,
					...(p.key ? { meta: p.key } : {}),
					href: `/projects/${encodeURIComponent(p.id)}`
				}));
			const memberHits: SearchHit[] = snapshot.members
				.filter(
					(m) =>
						m.agent?.status !== 'offboarded' &&
						(m.name.toLowerCase().includes(needle) ||
							(m.handle && m.handle.toLowerCase().includes(needle)))
				)
				.slice(0, MAX_HITS)
				.map((m) => ({
					id: m.id,
					title: m.name,
					meta: m.kind === 'agent' ? 'Agent' : 'Person',
					href: `/members/${encodeURIComponent(m.id)}`
				}));
			return {
				projects: projectHits,
				work: [],
				members: memberHits
			};
		}
	});
}
