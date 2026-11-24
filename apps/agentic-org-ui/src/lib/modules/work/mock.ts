import type { OrgApi } from '$lib/modules/organization/api';
import { isFormer } from '$lib/modules/organization/model';
import type { WorkAccepted } from '$lib/proto';
import type { WorkApi } from './api';
import {
	activityOf,
	applyWork,
	emptyWork,
	itemOf,
	positionOf,
	similarRequests,
	summaries,
	type WorkCall,
	type WorkContext,
	type WorkState
} from './engine';

/** The browser mock also lets a test act as another member, as an agent working through the CLI would. */
export interface MockWorkApi extends WorkApi {
	commandAs(organizationId: string, actor: string, call: WorkCall): Promise<WorkAccepted>;
}

export interface MockWorkStorage {
	getItem(key: string): string | null;
	setItem(key: string, value: string): void;
}

const key = (organizationId: string) => `agentic-org.mock.work.${organizationId}`;

export function createMockWork(
	org: Pick<OrgApi, 'view'>,
	{
		storage = globalThis.localStorage as MockWorkStorage | undefined,
		now = () => new Date().toISOString()
	}: { storage?: MockWorkStorage; now?: () => string } = {}
): MockWorkApi {
	const memory = new Map<string, WorkState>();
	const load = (organizationId: string): WorkState => {
		const held = memory.get(organizationId);
		if (held) return held;
		try {
			const text = storage?.getItem(key(organizationId));
			if (text) {
				const parsed = JSON.parse(text) as WorkState;
				if (parsed && typeof parsed.seq === 'number' && parsed.items && parsed.answers) {
					memory.set(organizationId, parsed);
					return parsed;
				}
			}
		} catch {
			// unreadable storage: start empty
		}
		const fresh = emptyWork();
		memory.set(organizationId, fresh);
		return fresh;
	};
	const save = (organizationId: string, state: WorkState) => {
		memory.set(organizationId, state);
		try {
			storage?.setItem(key(organizationId), JSON.stringify(state));
		} catch {
			// kept in memory only
		}
	};
	const context = async (organizationId: string, actor?: string): Promise<WorkContext> => {
		const { snapshot } = await org.view(organizationId);
		const owner = snapshot.members.find((m) => m.owner);
		return {
			now: now(),
			actor: actor ?? owner?.id ?? 'owner',
			projects: (snapshot.projects ?? []).map((p) => ({
				id: p.id,
				teamIds: (snapshot.project_teams ?? [])
					.filter((l) => l.project_id === p.id)
					.map((l) => l.team_id),
				// The project's areas are organization data (the `area.*` commands own them).
				areas: (snapshot.areas ?? [])
					.filter((a) => a.project_id === p.id)
					.map((a) => ({ id: a.id, general: a.general, deleted: a.deleted })),
				archived: p.archived
			})),
			members: snapshot.members.filter((m) => !isFormer(m)).map((m) => m.id)
		};
	};
	const apply = async (organizationId: string, call: WorkCall, actor?: string) => {
		const { state, accepted } = applyWork(
			load(organizationId),
			call,
			await context(organizationId, actor)
		);
		save(organizationId, state);
		return accepted;
	};
	return {
		requests: true,
		async snapshot(organizationId, projectId, closedSince) {
			const state = load(organizationId);
			return { position: positionOf(state), items: summaries(state, projectId, closedSince) };
		},
		async get(organizationId, itemId) {
			return itemOf(load(organizationId), itemId, now());
		},
		activity: async (organizationId, itemId) => activityOf(load(organizationId), itemId),
		similar: async (organizationId, query) => ({
			items: structuredClone(similarRequests(load(organizationId), query))
		}),
		command: (organizationId, call) => apply(organizationId, call),
		commandAs: (organizationId, actor, call) => apply(organizationId, call, actor)
	};
}
