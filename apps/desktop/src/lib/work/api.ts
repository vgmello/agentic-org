import type { OrgApi } from '$lib/org/api';
import { isFormer } from '$lib/org/model';
import type { WorkAccepted, WorkItem, WorkItemSummary, WorkPosition } from '$lib/proto';
import { applyWork, emptyWork, itemOf, positionOf, summaries, type WorkCall, type WorkContext, type WorkState } from './engine';

/**
 * What the work screens need from an office: `work.snapshot`, `work.get` and the item calls. The calls are
 * `org_proto::work` (ada's W7 design §4); until the real office serves them this is a mock that keeps the work in
 * the browser, like the first org mock did. Always sample data.
 */
export interface WorkApi {
	/** `closedSince`: Done tasks that went Done at or after it (`work.snapshot { closed_since }`); all of them when absent. */
	snapshot(organizationId: string, projectId?: string, closedSince?: string): Promise<{ position: WorkPosition; items: WorkItemSummary[] }>;
	get(organizationId: string, itemId: string): Promise<WorkItem>;
	/** Rejects with the error the office gave (an `AppError` with source `api`). */
	command(organizationId: string, call: WorkCall): Promise<WorkAccepted>;
}

export interface MockWorkStorage {
	getItem(key: string): string | null;
	setItem(key: string, value: string): void;
}

/** The id of a project's General area in the mock: areas are organization data that `area.*` commands will own. */
export const generalAreaId = (projectId: string) => `${projectId}-general`;

const key = (organizationId: string) => `agentic-org.mock.work.${organizationId}`;

export function createMockWork(
	org: Pick<OrgApi, 'view'>,
	{ storage = globalThis.localStorage as MockWorkStorage | undefined, now = () => new Date().toISOString() }: { storage?: MockWorkStorage; now?: () => string } = {}
): WorkApi {
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
	const context = async (organizationId: string): Promise<WorkContext> => {
		const { snapshot } = await org.view(organizationId);
		const owner = snapshot.members.find((m) => m.owner);
		return {
			now: now(),
			actor: owner?.id ?? 'owner',
			projects: (snapshot.projects ?? []).map((p) => ({
				id: p.id,
				teamIds: (snapshot.project_teams ?? []).filter((l) => l.project_id === p.id).map((l) => l.team_id),
				// Until the area commands exist every project has just General.
				areas: [{ id: generalAreaId(p.id), general: true }]
			})),
			members: snapshot.members.filter((m) => !isFormer(m)).map((m) => m.id)
		};
	};
	return {
		async snapshot(organizationId, projectId, closedSince) {
			const state = load(organizationId);
			return { position: positionOf(state), items: summaries(state, projectId, closedSince) };
		},
		async get(organizationId, itemId) {
			return itemOf(load(organizationId), itemId, now());
		},
		async command(organizationId, call) {
			const { state, accepted } = applyWork(load(organizationId), call, await context(organizationId));
			save(organizationId, state);
			return accepted;
		}
	};
}
