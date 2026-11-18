// Mock for src/lib/modules/search/api.ts: every match, from the org mock and the shell's work items.
import { orgApi } from '$lib/org/api';
import type { SearchPageApi } from '../../../src/lib/modules/search/api';
import type { SearchHit } from '../../../src/lib/modules/shell/types';
import { mockLatency, mockMode } from '../mock-mode';
import work from '../data/shell/work.json' with { type: 'json' };

const has = (q: string, ...t: (string | undefined)[]) =>
	t.some((x) => x?.toLowerCase().includes(q.toLowerCase()));

export const searchApi: SearchPageApi = {
	async results(organizationId, query) {
		const { snapshot } = await orgApi.view(organizationId);
		await mockLatency(300);
		if (mockMode().includes('error')) throw new Error('The office did not answer in time.');
		const projects: SearchHit[] = (snapshot.projects ?? [])
			.filter((p) => !p.archived && has(query, p.name, p.key))
			.map((p) => ({
				id: p.id,
				title: p.name,
				meta: p.key || undefined,
				href: `/projects/${p.id}`
			}));
		const items: SearchHit[] = work
			.filter((w) => has(query, w.ref, w.title))
			.map((w) => ({
				id: w.ref,
				title: w.title,
				meta: `${w.ref} · ${w.state}`,
				href: `/work/${w.ref}`
			}));
		const members: SearchHit[] = snapshot.members
			.filter((m) => has(query, m.name, m.agent?.title))
			.map((m) => ({
				id: m.id,
				title: m.name,
				meta: m.kind === 'agent' ? `Agent${m.agent?.title ? ` · ${m.agent.title}` : ''}` : 'Person',
				href: `/members/${m.id}`
			}));
		return { projects, work: items, members };
	}
};
