// Mock for src/lib/modules/home/api.ts under `bun run dev:mock`. Projects and members come from the org mock;
// the counts, who is working and the month line are JSON. Look at states with `?mock=noprojects`, `nopresence`,
// `nobody`, `nomonth` or `error` (the projects section fails).
import { orgApi } from '$lib/org/api';
import type { HomeApi } from '../../../src/lib/modules/home/api';
import { isPocDataset, mockLatency, mockMode } from '../mock-mode';
import { pocHomeProjects } from '../poc-dataset';
import summary from '../data/home/summary.json' with { type: 'json' };

const MINUTE = 60_000;
type Counts = { done: number; total: number; open: number; blocked: number; due?: string };

export const homeApi: HomeApi = {
	async projects(organizationId) {
		const { snapshot } = await orgApi.view(organizationId);
		await mockLatency();
		const mode = mockMode();
		if (mode.includes('error')) throw new Error('The office did not answer in time.');
		if (mode.includes('noprojects')) return [];
		if (isPocDataset()) return structuredClone(pocHomeProjects());
		return (snapshot.projects ?? [])
			.filter((p) => !p.archived)
			.map((p) => {
				const teams = (snapshot.project_teams ?? [])
					.filter((l) => l.project_id === p.id)
					.map((l) => l.team_id);
				const members = new Set(
					(snapshot.memberships ?? [])
						.filter((m) => teams.includes(m.team_id))
						.map((m) => m.member_id)
				);
				const counts: Counts = (summary.projects as Record<string, Counts>)[p.id] ?? {
					done: 0,
					total: 0,
					open: 0,
					blocked: 0
				};
				return { id: p.id, name: p.name, ...counts, members: members.size };
			})
			.sort((a, b) => a.name.localeCompare(b.name));
	},
	async workingNow(organizationId) {
		const { snapshot } = await orgApi.view(organizationId);
		await mockLatency(350);
		const mode = mockMode();
		if (mode.includes('nopresence')) return { live: false, agents: [] };
		if (mode.includes('nobody')) return { live: true, agents: [] };
		const base = Date.now();
		return {
			live: true,
			agents: summary.working.map((w) => ({
				member: {
					id: w.member,
					name: snapshot.members.find((m) => m.id === w.member)?.name ?? w.member
				},
				...('task' in w ? { task: w.task } : {}),
				since: new Date(base - w.since_ago * MINUTE).toISOString()
			}))
		};
	},
	async month() {
		await mockLatency(450);
		return mockMode().includes('nomonth') ? null : summary.month;
	}
};
