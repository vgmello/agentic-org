// Mock for src/lib/modules/inbox/api.ts under `bun run dev:mock`. Items are JSON with times given as minutes
// from now; people, teams and projects come from the org mock so IDs match the other pages. Actions change the
// list in memory. `?mock=more` adds four more open items, `?mock=empty` empties the Inbox, `?mock=error` fails
// the load and `?mock=live` resolves one item "in another client" a few seconds in.
import { orgApi } from '$lib/modules/organization/api';
import type { InboxApi } from '../../../src/lib/modules/inbox/api';
import type { InboxItem } from '../../../src/lib/modules/inbox/types';
import { mockLatency, mockMode, POC_ORG_ID } from '../mock-mode';
import { pocInboxItems } from '../poc-dataset';
import data from '../data/inbox/items.json' with { type: 'json' };

const MINUTE = 60_000;
type Raw = (typeof data.open)[number] & {
	ago: number;
	exp_in?: number;
	fail_first?: string;
	handled?: { outcome: string; by?: string; ago: number };
	recommendation?: string;
	team?: string;
	project?: string;
	open?: { path: string; label: string };
	choices?: { id: string; label: string }[];
};

const itemsByOrg = new Map<string, InboxItem[]>();
const pendingByOrg = new Map<string, Promise<InboxItem[]>>();
const failed = new Set<string>();
const refusals = new Map<string, string>();
const listenersByOrg = new Map<string, Set<() => void>>();
const liveTimers = new Map<string, ReturnType<typeof setTimeout>>();

async function load(organizationId: string): Promise<InboxItem[]> {
	const cached = itemsByOrg.get(organizationId);
	if (cached) return cached;
	const inflight = pendingByOrg.get(organizationId);
	if (inflight) return inflight;
	const promise = (async (): Promise<InboxItem[]> => {
		if (organizationId === POC_ORG_ID) {
			const next = pocInboxItems();
			itemsByOrg.set(organizationId, next);
			return next;
		}
		const { snapshot } = await orgApi.view(organizationId);
		const people = new Map(snapshot.members.map((m) => [m.id, m]));
		const teams = new Map(snapshot.teams.map((t) => [t.id, t.name]));
		const projects = new Map((snapshot.projects ?? []).map((p) => [p.id, p.name]));
		const base = Date.now();
		const raw = [
			...data.open,
			...(mockMode().includes('more') ? data.more : []),
			...data.handled
		] as Raw[];
		const next: InboxItem[] = raw.map((r) => {
			const member = people.get(r.who);
			if (r.fail_first) refusals.set(r.id, r.fail_first);
			return {
				id: r.id,
				kind: r.kind as InboxItem['kind'],
				source: r.source,
				title: r.title,
				who: { id: r.who, name: member?.name ?? r.who, kind: member?.kind ?? 'agent' },
				...(r.team ? { team: { id: r.team, name: teams.get(r.team) ?? r.team } } : {}),
				...(r.project
					? { project: { id: r.project, name: projects.get(r.project) ?? r.project } }
					: {}),
				why: r.why,
				...(r.recommendation ? { recommendation: r.recommendation } : {}),
				created_at: new Date(base - r.ago * MINUTE).toISOString(),
				...(r.exp_in ? { expires_at: new Date(base + r.exp_in * MINUTE).toISOString() } : {}),
				actions: r.actions as InboxItem['actions'],
				...(r.choices ? { choices: r.choices } : {}),
				...(r.open ? { open: r.open } : {}),
				state: r.state as InboxItem['state'],
				...(r.handled
					? {
							handled: {
								outcome: r.handled.outcome,
								...(r.handled.by ? { by: r.handled.by } : {}),
								at: new Date(base - r.handled.ago * MINUTE).toISOString()
							}
						}
					: {})
			};
		});
		itemsByOrg.set(organizationId, next);
		return next;
	})().finally(() => {
		pendingByOrg.delete(organizationId);
	});
	pendingByOrg.set(organizationId, promise);
	return promise;
}

const notify = (organizationId: string) =>
	listenersByOrg.get(organizationId)?.forEach((listener) => listener());

export const inboxApi: InboxApi = {
	async list(organizationId) {
		await mockLatency();
		const mode = mockMode();
		if (mode.includes('error')) throw new Error('The office did not answer in time.');
		const all = await load(organizationId);
		return structuredClone(mode.includes('empty') ? [] : all);
	},
	async act(organizationId, itemId, request) {
		const all = await load(organizationId);
		await mockLatency();
		const item = all.find((i) => i.id === itemId);
		if (!item) throw new Error('This item is gone.');
		if (item.state === 'handled') throw new Error('It was already resolved, so nothing changed.');
		const refusal = refusals.get(itemId);
		if (refusal && !failed.has(itemId)) {
			failed.add(itemId);
			throw new Error(refusal);
		}
		const outcome =
			request.type === 'approve'
				? 'Approved'
				: request.type === 'deny'
					? `Denied: ${request.reason}`
					: request.type === 'reply'
						? 'Answered'
						: request.type === 'choose'
							? `Decided: ${item.choices?.find((c) => c.id === request.choice)?.label ?? request.choice}`
							: 'Dismissed';
		Object.assign(item, {
			state: 'handled',
			actions: [],
			handled: { outcome, by: 'you', at: new Date().toISOString() }
		});
		notify(organizationId);
		return structuredClone(item);
	},
	async subscribe(organizationId, onChange) {
		await load(organizationId);
		let set = listenersByOrg.get(organizationId);
		if (!set) {
			set = new Set();
			listenersByOrg.set(organizationId, set);
		}
		set.add(onChange);
		if (mockMode().includes('live') && !liveTimers.has(organizationId)) {
			liveTimers.set(
				organizationId,
				setTimeout(() => {
					const orgItems = itemsByOrg.get(organizationId);
					const target = orgItems?.find((i) => i.id === 'inbox-2' && i.state === 'open');
					if (!target) return;
					Object.assign(target, {
						state: 'handled',
						actions: [],
						handled: {
							outcome: 'Answered in the command line',
							by: 'you',
							at: new Date().toISOString()
						}
					});
					notify(organizationId);
				}, 8000)
			);
		}
		return () => {
			set?.delete(onChange);
		};
	}
};
