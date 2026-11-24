// Mock for src/lib/modules/activity/api.ts under `bun run dev:mock`. Rows and threads are JSON with times given
// as minutes ago; people, teams and projects come from the org mock so IDs match the other pages. New rows
// "arrive" every ten seconds (six in all) so the live behaviour can be seen.
import { orgApi } from '$lib/modules/organization/api';
import type { ActivityApi } from '../../../src/lib/modules/activity/api';
import { matchesQuery } from '../../../src/lib/modules/activity/filter';
import type {
	ActivityActor,
	ActivityRow,
	ActivityRun,
	ActivityThread
} from '../../../src/lib/modules/activity/types';
import { mockLatency, mockMode, POC_ORG_ID } from '../mock-mode';
import { pocActivityRows } from '../poc-dataset';
import arrivals from '../data/activity/arrivals.json' with { type: 'json' };
import rowData from '../data/activity/rows.json' with { type: 'json' };
import threadData from '../data/activity/threads.json' with { type: 'json' };

const MINUTE = 60_000;
interface OrgActivityState {
	rows: ActivityRow[];
	people: Map<string, ActivityActor>;
	listeners: Set<(row: ActivityRow) => void>;
	arrived: number;
	timer?: ReturnType<typeof setInterval>;
}
const orgStates = new Map<string, OrgActivityState>();
const pendingLoads = new Map<
	string,
	Promise<{
		snapshot: Awaited<ReturnType<typeof orgApi.view>>['snapshot'];
		state: OrgActivityState;
	}>
>();

function arriveFor(organizationId: string, state: OrgActivityState) {
	if (organizationId === POC_ORG_ID) return;
	if (state.arrived >= arrivals.length || mockMode().includes('quiet')) return;
	const next = arrivals[state.arrived++];
	const row: ActivityRow = {
		id: `live-${state.arrived}`,
		at: new Date().toISOString(),
		actor: state.people.get(next.actor) ?? { id: next.actor, name: next.actor, kind: 'agent' },
		kind: next.kind as ActivityRow['kind'],
		action: next.action as ActivityRow['action'],
		text: next.text,
		...('preview' in next && next.preview ? { preview: next.preview as string } : {}),
		teamIds: next.teams,
		projectIds: next.projects,
		memberIds: next.members,
		...('thread' in next && next.thread
			? { thread: next.thread as { id: string; with: string } }
			: {})
	};
	state.rows = [row, ...state.rows];
	for (const listener of state.listeners) listener(row);
}

async function load(organizationId: string) {
	const existing = orgStates.get(organizationId);
	if (existing) {
		const { snapshot } = await orgApi.view(organizationId);
		return { snapshot, state: existing };
	}
	const inflight = pendingLoads.get(organizationId);
	if (inflight) return inflight;
	const promise = (async () => {
		const { snapshot } = await orgApi.view(organizationId);
		const people = new Map<string, ActivityActor>(
			snapshot.members.map((m) => [m.id, { id: m.id, name: m.name, kind: m.kind }])
		);
		let rows: ActivityRow[];
		if (organizationId === POC_ORG_ID) {
			rows = pocActivityRows();
		} else {
			const base = Date.now();
			const actor = (id: string): ActivityActor =>
				people.get(id) ?? { id, name: id, kind: 'agent' };
			rows = rowData.map((r) => {
				const data = r as typeof r & {
					run?: {
						id: string;
						member: string;
						state: string;
						start_ago: number;
						dur?: number;
						trigger: string;
						outcome?: string;
						log?: string[];
					};
				};
				let run: ActivityRun | undefined;
				if (data.run) {
					const started = base - data.run.start_ago * MINUTE;
					run = {
						id: data.run.id,
						memberId: data.run.member,
						state: data.run.state as ActivityRun['state'],
						started_at: new Date(started).toISOString(),
						...(data.run.dur
							? { ended_at: new Date(started + data.run.dur * MINUTE).toISOString() }
							: {}),
						trigger: data.run.trigger,
						...(data.run.outcome ? { outcome: data.run.outcome } : {}),
						...(data.run.log ? { log: data.run.log } : {})
					};
				}
				return {
					id: r.id,
					at: new Date(base - r.ago * MINUTE).toISOString(),
					actor: actor(r.actor),
					kind: r.kind as ActivityRow['kind'],
					action: r.action as ActivityRow['action'],
					text: r.text,
					...('preview' in r && r.preview ? { preview: r.preview as string } : {}),
					...('href' in r && r.href ? { href: r.href as ActivityRow['href'] } : {}),
					teamIds: r.teams ?? [],
					projectIds: r.projects ?? [],
					memberIds: r.members ?? [],
					...('status' in r && r.status ? { status: true } : {}),
					...('thread' in r && r.thread
						? { thread: r.thread as { id: string; with: string } }
						: {}),
					...(run ? { run } : {})
				};
			});
		}
		const state: OrgActivityState = {
			rows,
			people,
			listeners: new Set(),
			arrived: 0
		};
		orgStates.set(organizationId, state);
		return { snapshot, state };
	})().finally(() => {
		pendingLoads.delete(organizationId);
	});
	pendingLoads.set(organizationId, promise);
	return promise;
}

export const activityApi: ActivityApi = {
	async feed(organizationId, query, limit) {
		const { state } = await load(organizationId);
		await mockLatency();
		const mode = mockMode();
		if (mode.includes('error')) throw new Error('The office did not answer in time.');
		const empty = mode.includes('empty');
		const matching = empty
			? []
			: state.rows
					.filter((row) => matchesQuery(row, query))
					.sort((a, b) => b.at.localeCompare(a.at));
		return {
			rows: matching.slice(0, limit),
			more: Math.max(0, matching.length - limit),
			organizationEmpty: empty,
			sources: {
				messaging: mode.includes('messaging') ? 'not-connected' : 'connected',
				git: mode.includes('git') ? 'paused' : 'ok'
			}
		};
	},
	async options(organizationId) {
		const { snapshot } = await load(organizationId);
		return {
			teams: snapshot.teams
				.map(({ id, name }) => ({ id, name }))
				.sort((a, b) => a.name.localeCompare(b.name)),
			members: snapshot.members
				.map(({ id, name }) => ({ id, name }))
				.sort((a, b) => a.name.localeCompare(b.name)),
			projects: (snapshot.projects ?? [])
				.filter((p) => !p.archived)
				.map(({ id, name }) => ({ id, name }))
				.sort((a, b) => a.name.localeCompare(b.name))
		};
	},
	async thread(organizationId, threadId): Promise<ActivityThread> {
		const { state } = await load(organizationId);
		await mockLatency();
		if (mockMode().includes('threaderror'))
			throw new Error('The office could not read this thread.');
		const found = (
			threadData as Record<
				string,
				{
					counterpart: string;
					owner_may_reply: boolean;
					messages: { from: string; ago: number; body: string | null }[];
				}
			>
		)[threadId];
		if (!found) throw new Error('This thread is not available to you.');
		const base = Date.now();
		return {
			id: threadId,
			counterpart: state.people.get(found.counterpart) ?? {
				id: found.counterpart,
				name: found.counterpart,
				kind: 'agent'
			},
			ownerMayReply: found.owner_may_reply,
			messages: found.messages.map((m, index) => ({
				id: `${threadId}-${index}`,
				at: new Date(base - m.ago * MINUTE).toISOString(),
				from: state.people.get(m.from) ?? { id: m.from, name: m.from, kind: 'agent' },
				...(m.body === null ? {} : { body: m.body })
			}))
		};
	},
	async subscribe(organizationId, onRow) {
		const { state } = await load(organizationId);
		state.listeners.add(onRow);
		if (organizationId !== POC_ORG_ID) {
			state.timer ??= setInterval(() => arriveFor(organizationId, state), 10_000);
		}
		return () => {
			state.listeners.delete(onRow);
			if (state.listeners.size === 0 && state.timer) {
				clearInterval(state.timer);
				state.timer = undefined;
			}
		};
	}
};
