// Mock for src/lib/modules/shell/api.ts under `bun run dev:mock`. Projects, teams and members come from the
// org mock (so the navigation and search match the pages); the Inbox count and work items are JSON.
import { orgApi } from '$lib/org/api';
import type { ShellApi } from '../../../src/lib/modules/shell/api';
import type { SearchHit } from '../../../src/lib/modules/shell/types';
import inbox from '../data/shell/inbox.json' with { type: 'json' };
import work from '../data/shell/work.json' with { type: 'json' };

const LIMIT = 5;
const matches = (query: string, ...texts: (string | undefined)[]) =>
	texts.some((text) => text?.toLowerCase().includes(query.toLowerCase()));

export const shellApi: ShellApi = {
	async navigation(organizationId) {
		const { snapshot } = await orgApi.view(organizationId);
		return {
			inboxOpen: inbox.open,
			projects: (snapshot.projects ?? [])
				.filter((p) => !p.archived)
				.map(({ id, name }) => ({ id, name })),
			teams: snapshot.teams.map(({ id, name }) => ({ id, name }))
		};
	},
	async search(organizationId, query) {
		const { snapshot } = await orgApi.view(organizationId);
		const projects: SearchHit[] = (snapshot.projects ?? [])
			.filter((p) => !p.archived && matches(query, p.name, p.key))
			.map((p) => ({
				id: p.id,
				title: p.name,
				meta: p.key || undefined,
				href: `/projects/${p.id}`
			}));
		const items: SearchHit[] = work
			.filter((w) => matches(query, w.ref, w.title))
			.map((w) => ({
				id: w.ref,
				title: w.title,
				meta: `${w.ref} · ${w.state}`,
				href: `/work/${w.ref}`
			}));
		const members: SearchHit[] = snapshot.members
			.filter((m) => matches(query, m.name, m.agent?.title))
			.map((m) => ({
				id: m.id,
				title: m.name,
				meta: m.kind === 'agent' ? 'Agent' : 'Person',
				href: `/members/${m.id}`
			}));
		return {
			projects: projects.slice(0, LIMIT),
			work: items.slice(0, LIMIT),
			members: members.slice(0, LIMIT)
		};
	}
};
