// The mock office's rules for feature requests (ada's W7 design §1.5, §1.10, §4.7): one state machine, New → Routed →
// Scoping → Scoped → Approved, or Declined / Merged from any open state. Like the rest of the mock engine these are
// pure functions over a plain state; the real office decides the same things with the same words.
import type { DocRef, DraftTask, Estimate, FeatureOutcome, ItemStatus, RequestOutcome, WorkItemSummary } from '$lib/proto';
import { REQUEST_LIMITS, WORK_LIMITS } from '$lib/proto';
import type { StoredTask, WorkCall, WorkContext, WorkState } from './engine';
import { emptyCounts, fail, idText, itemAt, officeLine, officeText, shortId, steps, touch } from './support';

export type RequestCall = Extract<WorkCall, { method: `request.${string}` }>;

/** The states a request can still move in. */
export const OPEN_REQUEST: readonly ItemStatus[] = ['new', 'routed', 'scoping', 'scoped'];

const PRIORITIES: readonly string[] = ['urgent', 'high', 'normal', 'low'];
const KINDS: readonly string[] = ['feature', 'change', 'technical', 'question'];
const ROUTED_BY: readonly string[] = ['person', 'rule', 'copilot'];

/** A bound of the mock only: the design gives no limit on an outcome's docs, only its 128 KiB. */
const MOCK_MAX_DOCS = 50;

/** The org-wide counter of request numbers: `REQ-n`, never reused (design §2.8). */
const COUNTER = 'req:';

function requestOf(state: WorkState, id: unknown): StoredTask {
	const item = itemAt(state, id);
	if (!item || item.summary.deleted) return fail('not_found', 'That request is not here.');
	if (item.summary.kind !== 'request') return fail('invalid_request', `That is a ${item.summary.kind}, not a request.`);
	return item;
}
const versioned = (item: StoredTask, version: number) => {
	// A version above the current one was never served (design §2.3). The mock's version is per item, not per field.
	if (version > item.summary.version) fail('invalid_request', 'That version of the request does not exist yet.');
	if (item.summary.version !== version) fail('conflict', 'This changed since you opened it. Reload to see the latest.');
};
const mustBeOpen = (item: StoredTask, allowed: readonly ItemStatus[], what: string) => {
	if (!allowed.includes(item.summary.status)) fail('invalid_request', `A request that is ${item.summary.status} can't be ${what}.`);
};
const reasonFor = (text: string | undefined, scoper: string | undefined): string | undefined => {
	if (text === undefined) return undefined;
	if (!scoper) fail('invalid_request', 'A reason goes with someone to route it to.');
	return officeLine(text, REQUEST_LIMITS.routeReason, 'The reason');
};
const knownKind = (kind: string | undefined) => {
	if (kind !== undefined && !KINDS.includes(kind)) fail('invalid_request', 'A request is a feature, change, technical or question.');
};
const knownRoutedBy = (by: string | undefined) => {
	if (by !== undefined && !ROUTED_BY.includes(by)) fail('invalid_request', 'A request is routed by a person, a rule or the copilot.');
};
function projectOf(ctx: WorkContext, id: string) {
	const project = ctx.projects.find((p) => p.id === id) ?? fail('not_found', 'That project is not here.');
	if (project.archived) fail('invalid_request', 'That project is archived.');
	return project;
}
function scoperOf(ctx: WorkContext, id: string | undefined): string | undefined {
	if (id === undefined) return undefined;
	if (!ctx.members.includes(id)) fail('unknown_recipient', 'That member cannot scope this.');
	return id;
}
const noteOn = (item: StoredTask, id: string, body: string, by: string, at: string, state: WorkState) => {
	if (Object.values(state.items).some((i) => i.comments?.some((c) => c.id === id))) fail('already_exists', 'That comment already exists.');
	(item.comments ??= []).push({ id, body, by, at });
};

const isObject = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const listOf = (value: unknown, what: string): unknown[] => (value === undefined ? [] : Array.isArray(value) ? value : fail('invalid_request', `${what} must be a list.`));

/** The outcome as the office keeps it: checked, trimmed and rebuilt from the fields it knows, nothing the caller added. */
function checkedOutcome(outcome: RequestOutcome, project: WorkContext['projects'][number] | undefined, ctx: WorkContext): RequestOutcome {
	if (!isObject(outcome)) return fail('invalid_request', 'A scoped request needs its outcome.');
	const rawDrafts = listOf(outcome.draft_tasks, 'The draft tasks');
	if (rawDrafts.length > REQUEST_LIMITS.drafts) fail('invalid_request', `An outcome can have at most ${REQUEST_LIMITS.drafts} draft tasks.`);
	const seen = new Set<string>();
	const draft_tasks: DraftTask[] = rawDrafts.map((raw) => {
		if (!isObject(raw)) return fail('invalid_request', 'A draft task must be an object.');
		const d = raw as unknown as DraftTask;
		const draftId = idText(d.draft_id, 'A draft id');
		if (seen.has(draftId)) fail('already_exists', 'Two drafts have the same id.');
		seen.add(draftId);
		const clean: DraftTask = { draft_id: draftId, title: officeLine(d.title, WORK_LIMITS.title, 'A draft title') };
		if (d.description !== undefined) clean.description = officeText(d.description, REQUEST_LIMITS.draftDescription, 'A draft description');
		if (d.priority !== undefined) {
			if (!PRIORITIES.includes(d.priority)) fail('invalid_request', 'The priority must be urgent, high, normal or low.');
			clean.priority = d.priority;
		}
		if (d.assignee_id) {
			if (!ctx.members.includes(d.assignee_id)) fail('unknown_recipient', 'That member cannot be assigned.');
			clean.assignee_id = d.assignee_id;
		}
		if (d.team_id) {
			if (project && !project.teamIds.includes(d.team_id)) fail('invalid_request', "That team isn't one of the project's teams.");
			clean.team_id = d.team_id;
		}
		return clean;
	});
	let feature: FeatureOutcome | undefined;
	if (outcome.feature !== undefined) {
		const f = outcome.feature;
		if (!isObject(f) || (f.type !== 'new' && f.type !== 'extends')) return fail('invalid_request', 'The feature must be new or extend one.');
		feature = { type: f.type, feature_id: idText(f.feature_id, 'The feature id') };
	}
	let estimate: Estimate | undefined;
	if (outcome.estimate !== undefined) {
		if (!isObject(outcome.estimate)) return fail('invalid_request', 'The estimate must be an object.');
		const { effort, cost_usd_cents_min: lo, cost_usd_cents_max: hi } = outcome.estimate as Estimate;
		for (const cents of [lo, hi]) if (cents !== undefined && (!Number.isSafeInteger(cents) || cents < 0)) fail('invalid_request', 'A cost is a whole number of cents.');
		if (lo !== undefined && hi !== undefined && lo > hi) fail('invalid_request', 'The lowest cost is above the highest.');
		estimate = {};
		if (effort !== undefined) estimate.effort = officeLine(effort, REQUEST_LIMITS.effort, 'The effort');
		if (lo !== undefined) estimate.cost_usd_cents_min = lo;
		if (hi !== undefined) estimate.cost_usd_cents_max = hi;
	}
	const rawDocs = listOf(outcome.docs, 'The docs');
	if (rawDocs.length > MOCK_MAX_DOCS) fail('invalid_request', `An outcome can link at most ${MOCK_MAX_DOCS} docs.`);
	const docs: DocRef[] = rawDocs.map((raw) => {
		if (!isObject(raw)) return fail('invalid_request', 'A doc must be an object.');
		const doc = raw as unknown as DocRef;
		const docKey = idText(doc.doc_key, 'A doc key');
		// The doc's project must exist; an archived project only stops new items, not links to what it holds.
		if (!ctx.projects.some((p) => p.id === doc.project_id)) fail('not_found', 'That project is not here.');
		return { project_id: doc.project_id, doc_key: docKey };
	});
	const clean: RequestOutcome = { docs, draft_tasks };
	if (feature) clean.feature = feature;
	if (estimate) clean.estimate = estimate;
	return clean;
}

/** `request.*`: changes `state` in place (the caller passes a copy) and pushes each item it changed onto `changed`. */
export function applyRequest(state: WorkState, call: RequestCall, ctx: WorkContext, changed: StoredTask[]): void {
	const { now, actor } = ctx;
	switch (call.method) {
		case 'request.create': {
			idText(call.item_id, 'item_id');
			if (itemAt(state, call.item_id)) fail('already_exists', 'That item already exists.');
			const text = officeText(call.text, REQUEST_LIMITS.text, 'The request');
			// Untitled: the first line of the words, cut to the title's length (a tab or other control reads as a space).
			const title = call.title !== undefined ? officeLine(call.title, WORK_LIMITS.title, 'The title') : [...text.split('\n')[0].replace(/[\p{Cc}\u2028\u2029]/gu, ' ').trim()].slice(0, WORK_LIMITS.title).join('').trim();
			knownKind(call.request_kind);
			knownRoutedBy(call.routed_by);
			if (call.project_id !== undefined) projectOf(ctx, call.project_id);
			const scoper = scoperOf(ctx, call.scoper_id);
			const reason = reasonFor(call.route_reason, scoper);
			const number = (state.next[COUNTER] ?? 0) + 1;
			state.next[COUNTER] = number;
			const item: StoredTask = {
				summary: {
					id: call.item_id,
					kind: 'request',
					project_id: call.project_id,
					number,
					short_id: shortId(call.item_id),
					title,
					status: scoper ? 'routed' : 'new',
					assignee_id: scoper,
					created_by: actor,
					created_at: now,
					updated_at: now,
					severity_confirmed: false,
					reopen_count: 0,
					request_kind: call.request_kind,
					fixes: [],
					fixed_by: [],
					checklist: emptyCounts([]),
					done_anyway: false,
					owner_office_id: 'hq',
					generation: 1,
					version: 1,
					deleted: false
				},
				text,
				route_reason: reason,
				routed_by: scoper ? call.routed_by : undefined,
				checklist: [],
				watchers: [actor]
			};
			state.items[call.item_id] = item;
			state.seq += 1;
			changed.push(item);
			return;
		}
		case 'request.update': {
			const item = requestOf(state, call.item_id);
			versioned(item, call.expected_version);
			mustBeOpen(item, OPEN_REQUEST, 'changed');
			if (call.project_id !== undefined && call.clear_project) fail('invalid_request', 'Give a project or clear it, not both.');
			if (call.title === undefined && call.request_kind === undefined && call.project_id === undefined && !call.clear_project) fail('invalid_request', 'Nothing to change.');
			if (call.title !== undefined) item.summary.title = officeLine(call.title, WORK_LIMITS.title, 'The title');
			if (call.request_kind !== undefined) {
				knownKind(call.request_kind);
				item.summary.request_kind = call.request_kind;
			}
			if (call.project_id !== undefined) {
				projectOf(ctx, call.project_id);
				item.summary.project_id = call.project_id;
			}
			if (call.clear_project) item.summary.project_id = undefined;
			touch(state, item, now);
			changed.push(item);
			return;
		}
		case 'request.route': {
			const item = requestOf(state, call.item_id);
			versioned(item, call.expected_version);
			// Sending it nowhere only undoes a route; a New request is already Not routed.
			mustBeOpen(item, call.scoper_id === undefined ? ['routed', 'scoping'] : ['new', 'routed', 'scoping'], call.scoper_id === undefined ? 'taken back' : 'sent to someone else');
			knownRoutedBy(call.routed_by);
			const scoper = scoperOf(ctx, call.scoper_id);
			item.route_reason = reasonFor(call.route_reason, scoper);
			item.routed_by = scoper ? call.routed_by : undefined;
			item.summary.assignee_id = scoper;
			// Sending it to someone makes it Routed again for them; sending it nowhere makes it Not routed.
			item.summary.status = scoper ? 'routed' : 'new';
			touch(state, item, now);
			changed.push(item);
			return;
		}
		case 'request.accept': {
			const item = requestOf(state, call.item_id);
			versioned(item, call.expected_version);
			mustBeOpen(item, ['new', 'routed'], 'accepted');
			// The caller becomes the scoper.
			item.summary.assignee_id = actor;
			item.summary.status = 'scoping';
			touch(state, item, now);
			changed.push(item);
			return;
		}
		case 'request.scope': {
			const item = requestOf(state, call.item_id);
			versioned(item, call.expected_version);
			mustBeOpen(item, ['routed', 'scoping'], 'scoped');
			const project = item.summary.project_id ? ctx.projects.find((p) => p.id === item.summary.project_id) : undefined;
			item.outcome = checkedOutcome(call.outcome, project, ctx);
			item.summary.status = 'scoped';
			touch(state, item, now);
			changed.push(item);
			return;
		}
		case 'request.ask_changes': {
			const item = requestOf(state, call.item_id);
			versioned(item, call.expected_version);
			mustBeOpen(item, ['scoped'], 'sent back for changes');
			const note = officeText(call.note, WORK_LIMITS.text, 'The note');
			noteOn(item, idText(call.comment_id, 'comment_id'), note, actor, now, state);
			item.summary.status = 'scoping';
			touch(state, item, now);
			changed.push(item);
			return;
		}
		case 'request.approve': {
			const item = requestOf(state, call.item_id);
			versioned(item, call.expected_version);
			mustBeOpen(item, ['scoped'], 'approved');
			const projectId = item.summary.project_id;
			if (!projectId) fail('invalid_request', 'Pick a project before approving: the tasks need one.');
			const project = projectOf(ctx, projectId!);
			const tasks = call.tasks;
			if (!Array.isArray(tasks) || tasks.length < 1 || tasks.length > REQUEST_LIMITS.drafts || !tasks.every(isObject)) fail('invalid_request', `Approve needs 1 to ${REQUEST_LIMITS.drafts} tasks.`);
			const feature = item.outcome?.feature;
			const featureId = feature && feature.type !== 'unknown' ? feature.feature_id : undefined;
			const links = new Set(Object.values(state.items).flatMap((i) => (i.links ?? []).map((l) => l.id)));
			const made: StoredTask[] = [];
			for (const t of tasks) {
				idText(t.item_id, 'A task id');
				idText(t.link_id, 'A link id');
				if (itemAt(state, t.item_id) || made.some((m) => m.summary.id === t.item_id)) fail('already_exists', 'That item already exists.');
				if (links.has(t.link_id)) fail('already_exists', 'That link already exists.');
				links.add(t.link_id);
				const title = officeLine(t.title, WORK_LIMITS.title, 'A task title');
				if (t.priority !== undefined && !PRIORITIES.includes(t.priority)) fail('invalid_request', 'The priority must be urgent, high, normal or low.');
				if (t.team_id && !project.teamIds.includes(t.team_id)) fail('invalid_request', "That team isn't one of the project's teams.");
				if (t.assignee_id && !ctx.members.includes(t.assignee_id)) fail('unknown_recipient', 'That member cannot be assigned.');
				const checklist = t.checklist?.length ? steps(t.checklist, []) : [];
				const number = (state.next[projectId!] ?? 0) + 1;
				state.next[projectId!] = number;
				const summary: WorkItemSummary = {
					id: t.item_id,
					kind: 'task',
					project_id: projectId,
					number,
					short_id: shortId(t.item_id),
					title,
					status: 'todo',
					priority: t.priority ?? 'normal',
					assignee_id: t.assignee_id,
					created_by: actor,
					created_at: now,
					updated_at: now,
					team_id: t.team_id,
					feature_id: featureId,
					severity_confirmed: false,
					reopen_count: 0,
					fixes: [],
					fixed_by: [],
					checklist: emptyCounts(checklist),
					done_anyway: false,
					owner_office_id: 'hq',
					generation: 1,
					version: 1,
					deleted: false
				};
				const task: StoredTask = {
					summary,
					description: t.description ? officeText(t.description, WORK_LIMITS.description, 'The description') : undefined,
					definition_of_done: t.definition_of_done ? officeText(t.definition_of_done, WORK_LIMITS.definitionOfDone, 'The definition of done') : undefined,
					checklist,
					watchers: [actor],
					links: [{ id: t.link_id, kind: 'from_request', to: item.summary.id, created_by: actor, created_at: now }]
				};
				made.push(task);
			}
			for (const task of made) {
				state.items[task.summary.id] = task;
				state.seq += 1;
				changed.push(task);
			}
			item.summary.status = 'approved';
			item.done_at = now;
			touch(state, item, now);
			changed.unshift(item);
			return;
		}
		case 'request.decline': {
			const item = requestOf(state, call.item_id);
			versioned(item, call.expected_version);
			mustBeOpen(item, OPEN_REQUEST, 'declined');
			item.decline_reason = officeText(call.reason, REQUEST_LIMITS.declineReason, 'The reason');
			item.summary.status = 'declined';
			item.done_at = now;
			touch(state, item, now);
			changed.push(item);
			return;
		}
		case 'request.merge': {
			const item = requestOf(state, call.item_id);
			versioned(item, call.expected_version);
			mustBeOpen(item, OPEN_REQUEST, 'merged');
			const target = requestOf(state, call.into);
			if (target === item) fail('invalid_request', "A request can't be merged into itself.");
			if (!OPEN_REQUEST.includes(target.summary.status)) fail('invalid_request', `REQ-${target.summary.number} is ${target.summary.status}, so nothing can be merged into it.`);
			const id = idText(call.comment_id, 'comment_id');
			// The target quotes this request's words (cut so the note, with its pointer, fits 10,000 characters) and its creator watches it.
			const words = item.text ?? '';
			const marker = `\n(continued in REQ-${item.summary.number})`;
			const quoted = [...words].length > WORK_LIMITS.text ? `${[...words].slice(0, WORK_LIMITS.text - [...marker].length).join('')}${marker}` : words;
			noteOn(target, id, quoted, actor, now, state);
			if (!target.watchers.includes(item.summary.created_by)) target.watchers.push(item.summary.created_by);
			item.merged_into = target.summary.id;
			item.summary.status = 'merged';
			item.done_at = now;
			touch(state, item, now);
			touch(state, target, now);
			changed.push(item, target);
			return;
		}
	}
}
