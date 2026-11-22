// Mock for src/lib/modules/inbox/api.ts under `bun run dev:mock`. Items are JSON with times given as minutes
// from now; people, teams and projects come from the org mock so IDs match the other pages. Actions change the
// list in memory. `?mock=more` adds four more open items, `?mock=empty` empties the Inbox, `?mock=error` fails
// the load and `?mock=live` resolves one item "in another client" a few seconds in.
import { orgApi } from '$lib/org/api';
import type { InboxApi } from '../../../src/lib/modules/inbox/api';
import type { InboxItem } from '../../../src/lib/modules/inbox/types';
import { isPocDataset, mockLatency, mockMode } from '../mock-mode';
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

let items: InboxItem[] | undefined;
let loadedDataset: 'poc' | 'default' | undefined;
const failed = new Set<string>();
const refusals = new Map<string, string>();
const listeners = new Set<() => void>();
let liveTimer: ReturnType<typeof setTimeout> | undefined;

async function load(organizationId: string): Promise<InboxItem[]> {
	const currentDataset = isPocDataset() ? 'poc' : 'default';
	if (items && loadedDataset === currentDataset) return items;
	loadedDataset = currentDataset;
	if (currentDataset === 'poc') {
		items = pocInboxItems();
		return items;
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
	items = raw.map((r) => {
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
	return items;
}

const notify = () => listeners.forEach((listener) => listener());

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
		await mockLatency(350);
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
		notify();
		return structuredClone(item);
	},
	async subscribe(organizationId, onChange) {
		await load(organizationId);
		listeners.add(onChange);
		if (mockMode().includes('live') && !liveTimer)
			liveTimer = setTimeout(() => {
				const target = items?.find((i) => i.id === 'inbox-2' && i.state === 'open');
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
				notify();
			}, 8000);
		return () => void listeners.delete(onChange);
	}
};
