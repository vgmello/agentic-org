import { describe, expect, it } from 'vitest';
import type { Member, OrgSnapshot, WorkEvent, WorkItemSummary } from '$lib/proto';
import { activityLines, ageWord, atName, filterActive, filterRequests, handleFor, NO_FILTER, similarRequests, withHandle, estimateWords, groupOf, groupRequests, nextAction, requestRef, requestsNeedingYou, stateWords, suggestScopers } from './requests-view';

const member = (id: string, name: string, title: string, extra: Partial<Member['agent']> = {}, kind: Member['kind'] = 'agent'): Member => ({
	id,
	kind,
	name,
	version: 1,
	agent: kind === 'agent' ? { role: title, title, runtime: 'claude_code', messaging_name: id, status: 'active', ...extra } : undefined
});
const snapshot = {
	members: [
		{ id: 'owner', kind: 'person', name: 'Sam', owner: true, version: 1 },
		member('eden', 'Eden', 'Product designer'),
		member('ada', 'ada', 'Staff engineer'),
		member('riley', 'Riley', 'Developer'),
		member('quinn', 'Quinn', 'Product designer', { status: 'paused' }),
		member('old', 'Old', 'Staff engineer', { status: 'offboarded' })
	],
	teams: [{ id: 'web', name: 'Web', lead_member_id: 'riley', version: 1 }],
	memberships: [{ id: 'm1', team_id: 'web', member_id: 'riley', version: 1 }],
	projects: [{ id: 'p1', name: 'Atlas', version: 1 }],
	project_teams: [{ id: 'pt', project_id: 'p1', team_id: 'web', version: 1 }]
} as unknown as OrgSnapshot;
const req = (status: WorkItemSummary['status'], extra: Partial<WorkItemSummary> = {}): WorkItemSummary =>
	({ id: `r-${status}`, kind: 'request', status, number: 1, short_id: 'abc123', title: 'T', deleted: false, ...extra }) as WorkItemSummary;

describe('REQ the queue', () => {
	it('puts each state in its group: scoped needs you, routed and scoping are with agents, new is not routed, ended ones are collapsed', () => {
		expect(['scoped', 'routed', 'scoping', 'new', 'approved', 'declined', 'merged', 'todo'].map((s) => groupOf({ status: s as WorkItemSummary['status'] }))).toEqual(['needs_you', 'with_agents', 'with_agents', 'not_routed', 'approved', 'closed', 'closed', undefined]);
	});

	it('lists only requests that are not deleted, newest number first, and counts what needs the owner', () => {
		const items = [req('new', { id: 'a', number: 3 }), req('new', { id: 'b', number: 9 }), req('scoped', { id: 'c', number: 5 }), req('new', { id: 'd', deleted: true }), { ...req('new'), id: 't', kind: 'task' } as WorkItemSummary, req('new', { id: 'p', number: undefined })];
		const groups = groupRequests(items);
		expect(groups.not_routed.map((i) => i.id)).toEqual(['p', 'b', 'a']);
		expect(groups.needs_you.map((i) => i.id)).toEqual(['c']);
		expect(requestsNeedingYou(items)).toBe(1);
	});
});

describe('REQ words', () => {
	it('names the state with who: sent, not accepted yet; scoping; scoped', () => {
		expect(stateWords(req('routed', { assignee_id: 'eden' }), snapshot)).toBe('Sent to Eden · not accepted yet');
		expect(stateWords(req('scoping', { assignee_id: 'eden' }), snapshot)).toBe('Eden is scoping');
		expect(stateWords(req('scoped', { assignee_id: 'eden' }), snapshot)).toBe('Scoped by Eden');
		expect(stateWords(req('new'), snapshot)).toBe('Not routed');
		expect(['approved', 'declined', 'merged'].map((s) => stateWords(req(s as 'approved'), snapshot))).toEqual(['Approved', 'Declined', 'Merged']);
	});

	it('writes an estimate as effort and a dollar range, or nothing', () => {
		const e = (estimate: object) => estimateWords({ docs: [], draft_tasks: [], estimate });
		expect(e({ effort: '3 days', cost_usd_cents_min: 4000, cost_usd_cents_max: 7000 })).toBe('3 days · ≈ $40–70');
		expect(e({ cost_usd_cents_min: 4050, cost_usd_cents_max: 4050 })).toBe('≈ $40.50');
		expect(e({ cost_usd_cents_min: 500 })).toBe('from $5');
		expect(e({ cost_usd_cents_max: 500 })).toBe('up to $5');
		expect(e({})).toBeUndefined();
		expect(estimateWords(undefined)).toBeUndefined();
		expect(estimateWords({ docs: [], draft_tasks: [] })).toBeUndefined();
	});

	it('writes the age in its largest unit and the reference with a prefix or as pending', () => {
		const now = '2026-11-09T12:00:00.000Z';
		expect([ageWord('2026-11-09T11:55:00.000Z', now), ageWord('2026-11-09T11:00:00.000Z', now), ageWord('2026-11-07T08:00:00.000Z', now), ageWord('2026-11-09T12:00:30.000Z', now)]).toEqual(['5 min', '1 h', '2 d', '0 min']);
		expect(requestRef({ number: 12, short_id: 'abc123' })).toBe('REQ-12');
		expect(requestRef({ number: undefined, short_id: 'abc123' })).toBe('Pending · abc123');
	});

	it('offers Review when scoped and Route when not routed, nothing otherwise', () => {
		expect(['scoped', 'new', 'routed', 'scoping', 'approved'].map((s) => nextAction({ status: s as 'new' }))).toEqual(['Review', 'Route', undefined, undefined, undefined]);
	});
});

describe('RQS-5 who to suggest without the copilot', () => {
	const names = (kind: 'feature' | 'change' | 'technical' | 'question', project?: string) => suggestScopers(snapshot, kind, project).map((s) => s.member.name);

	it('sends a feature or a change to the product designer, then the project lead, then the staff engineer', () => {
		expect(names('feature', 'p1')).toEqual(['Eden', 'Riley', 'ada']);
		expect(names('change')).toEqual(['Eden', 'ada', 'Riley']);
	});

	it('sends a technical request to the staff engineer, then the lead', () => {
		expect(names('technical', 'p1')).toEqual(['ada', 'Riley', 'Eden']);
	});

	it('sends a question to the project lead, or, without a project, to whoever is there', () => {
		expect(names('question', 'p1')).toEqual(['Riley', 'ada', 'Eden']);
		expect(names('question')).toEqual(['ada', 'Eden', 'Riley']);
	});

	it('never suggests a paused or offboarded agent, a person, or anyone twice, and says why in one line', () => {
		const all = suggestScopers(snapshot, 'feature', 'p1');
		expect(all.map((s) => s.member.id)).not.toContain('quinn');
		expect(all.map((s) => s.member.id)).not.toContain('old');
		expect(all.map((s) => s.member.id)).not.toContain('owner');
		expect(new Set(all.map((s) => s.member.id)).size).toBe(all.length);
		expect(all[0].why).toBe('product designer: a new capability for users');
		expect(all.every((s) => /: .+/.test(s.why))).toBe(true);
		expect(suggestScopers(snapshot, 'question', undefined).at(-1)?.why).toBe('developer: another agent who can take it');
		expect(suggestScopers({ ...snapshot, members: [snapshot.members[0]] }, 'feature', undefined)).toEqual([]);
	});
});

describe('REQ the conversation', () => {
	const ev = (change: WorkEvent['change'], actor = 'owner', version = 1): WorkEvent => ({ target_id: 'r', version, at: '2026-11-09T10:00:00.000Z', actor_id: actor, org_position: { epoch: 0, seq: 0 }, change });
	const lines = (events: WorkEvent[]) => activityLines(events, snapshot, 'owner').map((l) => [l.text, l.note]);
	const routed = (over: Partial<Extract<WorkEvent['change'], { type: 'routed' }>>, actor = 'owner') => ev({ type: 'routed', from: 'new', to: 'routed', ...over }, actor);

	it('reads each kind of event as a sentence, with you as You', () => {
		expect(
			lines([
				ev({ type: 'created' }),
				routed({ scoper_to: 'eden', reason: 'designer: a new capability for users' }),
				routed({ from: 'routed', scoper_from: 'eden', scoper_to: 'ada' }),
				routed({ from: 'routed', to: 'new', scoper_from: 'ada' }),
				ev({ type: 'fields_changed', fields: [{ field: 'title', from: 'a', to: 'b' }, { field: 'project_id', to: 'p1' }] }),
				routed({ from: 'routed', to: 'scoping', scoper_from: 'ada', scoper_to: 'ada' }, 'ada'),
				ev({ type: 'status_changed', from: 'scoping', to: 'scoped' }, 'ada'),
				ev({ type: 'status_changed', from: 'scoped', to: 'scoping', reason: 'Daily or weekly?' }),
				ev({ type: 'comment', comment_id: 'c', body: 'Dark mode please', comment_kind: 'note' }),
				ev({ type: 'status_changed', from: 'scoped', to: 'approved' }),
				ev({ type: 'status_changed', from: 'new', to: 'declined', reason: 'Not now' }),
				ev({ type: 'status_changed', from: 'new', to: 'merged', reason: 'Merged into REQ-4' }),
				{ ...ev({ type: 'unknown' }), actor_id: undefined }
			])
		).toEqual([
			['You raised this', undefined],
			['You sent this to Eden', 'designer: a new capability for users'],
			['You moved this from Eden to ada', undefined],
			['You took this back', undefined],
			['You changed the title and project', undefined],
			['ada accepted it', undefined],
			['ada scoped it', undefined],
			['You asked for changes', 'Daily or weekly?'],
			['You wrote', 'Dark mode please'],
			['You approved it', undefined],
			['You declined it', 'Not now'],
			['You merged it', 'Merged into REQ-4'],
			['The office changed it', undefined]
		]);
	});

	it('the note an ask for changes cites is shown once, on the "asked for changes" line, not also as "wrote"', () => {
		expect(
			lines([
				ev({ type: 'comment', comment_id: 'c1', body: 'Daily or weekly?', comment_kind: 'note' }),
				ev({ type: 'status_changed', from: 'scoped', to: 'scoping', reason: 'Daily or weekly?' }),
				ev({ type: 'comment', comment_id: 'c2', body: 'Daily or weekly?', comment_kind: 'note' }, 'ada'),
				ev({ type: 'comment', comment_id: 'c3', body: 'A different note', comment_kind: 'note' })
			])
		).toEqual([
			['You asked for changes', 'Daily or weekly?'],
			['ada wrote', 'Daily or weekly?'],
			['You wrote', 'A different note']
		]);
	});

	it('hides only the comment an ask for changes cites: the nearest one before it, not an earlier one with the same words', () => {
		expect(
			lines([
				ev({ type: 'comment', comment_id: 'c1', body: 'x', comment_kind: 'note' }),
				ev({ type: 'comment', comment_id: 'c2', body: 'x', comment_kind: 'note' }),
				ev({ type: 'status_changed', from: 'scoped', to: 'scoping', reason: 'x' })
			])
		).toEqual([
			['You wrote', 'x'],
			['You asked for changes', 'x']
		]);
	});

	it('taking up a request that was with someone else says both: it moved to you, and you accepted it', () => {
		expect(lines([routed({ from: 'routed', to: 'scoping', scoper_from: 'eden', scoper_to: 'owner' })])).toEqual([
			['You moved this from Eden to you', undefined],
			['You accepted it', undefined]
		]);
	});

	it('sending a request to the person it is already with still reads as sent to them', () => {
		expect(lines([routed({ from: 'routed', scoper_from: 'eden', scoper_to: 'eden', reason: 'again' })])).toEqual([['You sent this to Eden', 'again']]);
	});

	it('reads fields of any shape, old or new, without crashing or naming what it should not', () => {
		const odd = (fields: unknown) => ev({ type: 'fields_changed', fields: fields as never });
		expect(lines([odd(['title', 'project_id']), odd([null, 5, { nope: 1 }]), odd(undefined), odd('x'), odd([{ field: 'constructor' }, { field: 'toString' }, { field: '__proto__' }])])).toEqual([
			['You changed the title and project', undefined],
			['You changed it', undefined],
			['You changed it', undefined],
			['You changed it', undefined],
			['You changed the constructor and toString and __proto__', undefined]
		]);
	});

	it('never reads real FieldChange objects as [object Object]', () => {
		const text = activityLines([ev({ type: 'fields_changed', fields: [{ field: 'title', from: 'a', to: 'b' }, { field: 'request_kind', to: 'technical' }] })], snapshot, 'owner').map((l) => l.text);
		expect(text).toEqual(['You changed the title and kind']);
	});

	it('says "you" when the owner is the one moved to, and names someone who left', () => {
		expect(lines([routed({ from: 'routed', scoper_from: 'ada', scoper_to: 'owner' }, 'ada'), routed({ scoper_to: 'gone' }, 'ada')])).toEqual([
			['ada moved this from ada to you', undefined],
			['ada sent this to Someone who left', undefined]
		]);
	});
});

describe('RQS-4 @name routing', () => {
	const team = {
		...snapshot,
		members: [
			...snapshot.members,
			member('ac-product-designer-juno', 'Juno Park', 'Product designer', { messaging_name: 'ac-product-designer-juno' }),
			member('juno-two', 'Juno Reyes', 'Developer', { messaging_name: 'juno-reyes' })
		]
	} as unknown as OrgSnapshot;
	const at = (text: string) => atName(text, team);
	const who = (text: string) => {
		const a = at(text);
		return a?.kind === 'exact' ? a.member.id : a ? a.matches.map((m) => m.id) : undefined;
	};

	it('takes a messaging name, a display name as a slug, or a first name only one agent has', () => {
		expect(who('@ac-product-designer-juno Let me capture requests')).toBe('ac-product-designer-juno');
		expect(who('@juno-reyes please')).toBe('juno-two');
		expect(who('@Eden costs per project')).toBe('eden');
		expect(who('  @ada')).toBe('ada');
	});

	it('a first name two agents share is not a pick: both are offered', () => {
		expect(who('@juno do it')).toEqual(['ac-product-designer-juno', 'juno-two']);
	});

	it('offers the agents a half-typed handle could become, and none for a stranger', () => {
		expect(who('@ed')).toEqual(['eden']);
		expect(who('@')).toEqual(['eden', 'ada', 'riley', 'quinn', 'ac-product-designer-juno']);
		expect(who('@zzz')).toBeUndefined();
	});

	it('is only for the start of the words: an @ later, an email, or no @ is not a route', () => {
		expect([at('ask @eden later'), at('write to eden@example.com'), at('plain words')]).toEqual([undefined, undefined, undefined]);
	});

	it('never offers a former agent, and never a person', () => {
		expect(who('@old')).toBeUndefined();
		expect(who('@sam')).toBeUndefined();
	});

	it('takes the handle up to punctuation and accents, and refuses @@', () => {
		expect([who('@Eden: costs per project'), who('@eden, please'), who('@eden.'), who('@Éden'), who('@@eden')]).toEqual(['eden', 'eden', 'eden', 'eden', undefined]);
	});

	it('a handle with $ in it is put in as typed, not as a replacement pattern', () => {
		const odd = member('x', "A$&B$'C", 'Developer', { messaging_name: undefined as unknown as string });
		expect(withHandle('@x rest', odd)).toBe("@a$&b$'c rest");
	});

	it('replaces the handle at the start with the messaging name and leaves the rest as typed', () => {
		const juno = team.members.find((m) => m.id === 'ac-product-designer-juno')!;
		expect(withHandle('@jun make it work @ada', juno)).toBe('@ac-product-designer-juno make it work @ada');
		expect(withHandle('  @jun x', juno)).toBe('  @ac-product-designer-juno x');
		expect(handleFor(juno)).toBe('ac-product-designer-juno');
	});
});

describe('RQS-13 similar requests', () => {
	const items = [
		req('new', { id: 'a', number: 9, title: 'Dark mode for the docs reader' }),
		req('scoping', { id: 'b', number: 10, title: 'Checklists on tasks' }),
		req('approved', { id: 'c', number: 11, title: 'Dark mode everywhere' }),
		req('new', { id: 'd', number: 12, title: 'Export a feature as PDF', deleted: true }),
		{ ...req('new', { id: 't', number: 13, title: 'Dark mode task' }), kind: 'task' } as WorkItemSummary
	];

	it('finds open requests that share meaningful words, best first, and never closed ones or other kinds', () => {
		expect(similarRequests('I want a dark mode for reading the docs', items).map((i) => i.id)).toEqual(['a']);
		expect(similarRequests('Checklists for tasks please', items).map((i) => i.id)).toEqual(['b']);
	});

	it('needs two shared words, or all the words of a short request, and offers at most two', () => {
		expect(similarRequests('something about the docs', items)).toEqual([]);
		expect(similarRequests('dark mode', [req('new', { id: 'x', title: 'Dark mode' })]).map((i) => i.id)).toEqual(['x']);
		// One shared word is not enough, even for a one-word title.
		expect(similarRequests('please export everything now', [req('new', { id: 'y', title: 'Export' })])).toEqual([]);
		// The @handle at the start is not part of the words.
		expect(similarRequests('@ac-product-designer-eden please', [req('new', { id: 'z', title: 'Product designer onboarding' })])).toEqual([]);
		const many = Array.from({ length: 5 }, (_, n) => req('new', { id: `m${n}`, number: n + 1, title: 'Weekly digest by email' }));
		expect(similarRequests('a weekly digest by email', many)).toHaveLength(2);
		expect(similarRequests('x', items)).toEqual([]);
	});
});

describe('RQS-7 the queue filters', () => {
	const items = [
		req('new', { id: 'a', number: 1, title: 'Dark mode', project_id: 'p1' }),
		req('routed', { id: 'b', number: 2, title: 'Weekly digest', assignee_id: 'eden' }),
		req('scoped', { id: 'c', number: 3, title: 'Costs per project', assignee_id: 'eden', project_id: 'p1' }),
		req('approved', { id: 'd', number: 4, title: 'Checklists', assignee_id: 'ada' })
	];
	const ids = (f: Partial<typeof NO_FILTER>) => filterRequests(items, { ...NO_FILTER, ...f }).map((i) => i.id);

	it('shows everything with no filter, and only what is still going when Open', () => {
		expect(ids({})).toEqual(['a', 'b', 'c', 'd']);
		expect(ids({ state: 'open' })).toEqual(['a', 'b', 'c']);
	});

	it('filters by who scopes it (or nobody) and by project (or none)', () => {
		expect(ids({ who: 'eden' })).toEqual(['b', 'c']);
		expect(ids({ who: 'none' })).toEqual(['a']);
		expect(ids({ project: 'p1' })).toEqual(['a', 'c']);
		expect(ids({ project: 'none' })).toEqual(['b', 'd']);
	});

	it('searches the title and the reference, every word must match, without case', () => {
		expect(ids({ query: 'COSTS' })).toEqual(['c']);
		expect(ids({ query: 'req-2' })).toEqual(['b']);
		expect(ids({ query: 'digest weekly' })).toEqual(['b']);
		expect(ids({ query: 'digest costs' })).toEqual([]);
		expect(ids({ query: '   ' })).toEqual(['a', 'b', 'c', 'd']);
	});

	it('knows when a filter is on', () => {
		expect([filterActive(NO_FILTER), filterActive({ ...NO_FILTER, state: 'open' }), filterActive({ ...NO_FILTER, who: 'x' }), filterActive({ ...NO_FILTER, query: ' a ' }), filterActive({ ...NO_FILTER, query: '  ' })]).toEqual([false, true, true, true, false]);
	});
});
