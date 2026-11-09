// The mock office's work-item rules for tasks and their checklists (ada's W7 design §1.10, §2, §4.4, §4.7), as pure
// functions over a plain state. It stands in for the office until `org_proto::work` exists; bugs, requests, links and
// comments come later. Errors are thrown as `AppError`s with the codes the design names.
import type {
	AppError,
	BugAssign,
	BugClose,
	BugMarkFixed,
	BugReopen,
	BugReport,
	BugTriage,
	BugUpdate,
	BugVerify,
	RequestAccept,
	RequestApprove,
	RequestAskChanges,
	RequestCreate,
	RequestDecline,
	RequestMerge,
	RequestOutcome,
	RequestRoute,
	RequestScope,
	RequestUpdate,
	RoutedBy,
	WorkLinkCall,
	WorkUnlink,
	WorkCommentCall,
	ApiErrorCode,
	ChecklistStep,
	NewStep,
	TaskAssign,
	TaskCreate,
	TaskSetStatus,
	TaskUpdate,
	TimeFigures,
	WorkAccepted,
	WorkItem,
	WorkEvent,
	WorkItemSummary,
	WorkListQuery,
	WorkPosition
} from '$lib/proto';
import { WORK_LIMITS } from '$lib/proto';
import { applyComment, applyRequest } from './requests';
import { emptyCounts, fail, idText, itemAt, longText, oneLine, shortId, steps, touch } from './support';

export { emptyCounts };

export interface StoredTask {
	summary: WorkItemSummary;
	description?: string;
	definition_of_done?: string;
	done_anyway?: { reason: string; steps: string[] };
	/** Bugs: what was reported and how it ended. */
	what_happened?: string;
	expected?: string;
	steps?: string;
	environment?: string;
	closed_reason?: string;
	/** When it first went In progress, and when it last went Done (a round closes at Done). */
	started_at?: string;
	done_at?: string;
	checklist: ChecklistStep[];
	watchers: string[];
	/** Links this item makes (`fixes`: a task to the bug it fixes). */
	links?: { id: string; kind: 'fixes' | 'from_request'; to: string; created_by: string; created_at: string }[];
	/** Requests: the original words, why this scoper, how it was routed, and how it ended. */
	text?: string;
	route_reason?: string;
	routed_by?: RoutedBy;
	outcome?: RequestOutcome;
	merged_into?: string;
	decline_reason?: string;
	/** Requests: what happened to it, oldest first (`work.activity`). */
	events?: WorkEvent[];
}

export interface WorkState {
	incarnation: number;
	seq: number;
	/** Next task number per project. */
	next: Record<string, number>;
	items: Record<string, StoredTask>;
	/** Answers to change ids already applied, so a repeat changes nothing. */
	answers: Record<string, { call: string; accepted: WorkAccepted }>;
}

export const emptyWork = (): WorkState => ({ incarnation: 1, seq: 0, next: {}, items: {}, answers: {} });

/** What the engine needs to know about the world around the items. */
export interface WorkContext {
	now: string;
	/** The bound member making the call. */
	actor: string;
	projects: { id: string; archived?: boolean; teamIds: string[]; areas?: { id: string; general?: boolean; deleted?: boolean }[] }[];
	/** Members who can be assigned (not offboarded). */
	members: string[];
}

export type WorkCall =
	| ({ method: 'task.create' } & TaskCreate)
	| ({ method: 'task.update' } & TaskUpdate)
	| ({ method: 'task.assign' } & TaskAssign)
	| ({ method: 'task.set_status' } & TaskSetStatus)
	| ({ method: 'bug.report' } & BugReport)
	| ({ method: 'bug.update' } & BugUpdate)
	| ({ method: 'bug.triage' } & BugTriage)
	| ({ method: 'bug.assign' } & BugAssign)
	| ({ method: 'bug.mark_fixed' } & BugMarkFixed)
	| ({ method: 'bug.verify' } & BugVerify)
	| ({ method: 'bug.close' } & BugClose)
	| ({ method: 'bug.reopen' } & BugReopen)
	| ({ method: 'request.create' } & RequestCreate)
	| ({ method: 'request.update' } & RequestUpdate)
	| ({ method: 'request.route' } & RequestRoute)
	| ({ method: 'request.accept' } & RequestAccept)
	| ({ method: 'request.scope' } & RequestScope)
	| ({ method: 'request.ask_changes' } & RequestAskChanges)
	| ({ method: 'request.approve' } & RequestApprove)
	| ({ method: 'request.decline' } & RequestDecline)
	| ({ method: 'request.merge' } & RequestMerge)
	| ({ method: 'work.link' } & WorkLinkCall)
	| ({ method: 'work.unlink' } & WorkUnlink)
	| ({ method: 'work.comment' } & WorkCommentCall)
	| { method: 'checklist.add'; change_id: string; item_id: string; steps: NewStep[]; after_step_id?: string }
	| { method: 'checklist.check'; change_id: string; step_id: string; done: boolean }
	| { method: 'checklist.edit'; change_id: string; step_id: string; expected_version: number; text?: string; required?: boolean }
	| { method: 'checklist.remove'; change_id: string; step_id: string; expected_version: number }
	| { method: 'work.delete'; change_id: string; item_id: string; expected_version: number };

const SEVERITY_IDS: readonly string[] = ['critical', 'major', 'minor', 'trivial'];
/** The free text of a bug report: up to 10,000 characters; a required text can't be empty. */
const bugText = (text: string | undefined, what: string, required: boolean): string => {
	const t = (text ?? '').trim();
	if (required && t === '') fail('invalid_request', `${what} can't be empty.`);
	return longText(t, WORK_LIMITS.text, what);
};

function find(state: WorkState, id: string): StoredTask {
	const item = itemAt(state, id);
	if (!item || item.summary.deleted) fail('not_found', 'That item is not here.');
	return item;
}
function stepOwner(state: WorkState, stepId: string): StoredTask {
	const owner = Object.values(state.items).find((i) => !i.summary.deleted && i.checklist.some((s) => s.id === stepId));
	return owner ?? fail('not_found', 'That checklist step is not here.');
}
function findKind(state: WorkState, id: string, kind: 'task' | 'bug'): StoredTask {
	const item = find(state, id);
	if (item.summary.kind !== kind) fail('invalid_request', `That is a ${item.summary.kind}, not a ${kind}.`);
	return item;
}
const expect = (item: StoredTask, version: number) => {
	if (item.summary.version !== version) fail('conflict', 'This changed since you opened it. Reload to see the latest.');
};

/** What just happened to a fix task or its bug: the rules of design §2.4 fire on these edges, never on a scan. */
type FixTrigger = { kind: 'task'; from: string; to: string } | { kind: 'linked'; taskStatus: string } | { kind: 'triaged' };

/**
 * A bug follows its fix tasks (design §2.4). While Triaged it goes In progress when a fix task goes In progress (or an In
 * progress task is linked, or the bug is triaged while one is In progress); it goes Fixed when a fix task goes Done (or
 * the bug is triaged) and every fix task is Done, at least one Completed, and no required step is left; it goes back to
 * In progress when a fix task goes back from Done while the bug is Fixed. Cancelled-only fix tasks and unticked required
 * steps change nothing, and neither does linking to a Fixed bug or unlinking.
 */
function followFixTasks(state: WorkState, bug: StoredTask, now: string, changed: StoredTask[], trigger: FixTrigger) {
	if (bug.summary.deleted || bug.summary.kind !== 'bug') return;
	const fixTasks = bug.summary.fixed_by.map((id) => state.items[id]).filter((t) => t && !t.summary.deleted);
	const status = bug.summary.status;
	let to: 'in_progress' | 'fixed' | undefined;
	const started = trigger.kind === 'task' ? trigger.to === 'in_progress' : trigger.kind === 'linked' ? trigger.taskStatus === 'in_progress' : fixTasks.some((t) => t.summary.status === 'in_progress');
	if (status === 'triaged' && started) to = 'in_progress';
	const finished = (trigger.kind === 'task' && trigger.to === 'done') || trigger.kind === 'triaged';
	if (finished && (status === 'triaged' || status === 'in_progress') && fixTasks.length > 0 && fixTasks.every((t) => t.summary.status === 'done') && fixTasks.some((t) => t.summary.resolution === 'completed')) {
		if (!bug.checklist.some((c) => c.required && !c.done)) to = 'fixed';
	}
	if (status === 'fixed' && trigger.kind === 'task' && trigger.from === 'done' && trigger.to !== 'done') to = 'in_progress';
	if (!to) return;
	bug.summary.status = to;
	touch(state, bug, now);
	if (!changed.includes(bug)) changed.push(bug);
}

function addFixLink(state: WorkState, task: StoredTask, linkId: string, bugId: string, changed: StoredTask[], now: string, actor: string) {
	if (typeof linkId !== 'string' || linkId === '' || linkId.length > 128) fail('invalid_request', 'link_id must be a text of 1 to 128 characters.');
	if (typeof bugId !== 'string') fail('not_found', 'That bug is not here.');
	const bug = itemAt(state, bugId);
	if (!bug || bug.summary.deleted || bug.summary.kind !== 'bug') fail('not_found', 'That bug is not here.');
	if (bug.summary.project_id !== task.summary.project_id) fail('invalid_request', 'A fix task and its bug must be in the same project.');
	if (Object.values(state.items).some((i) => i.links?.some((l) => l.id === linkId))) fail('already_exists', 'That link already exists.');
	if (task.summary.fixes.includes(bugId)) fail('already_exists', 'That task already fixes that bug.');
	(task.links ??= []).push({ id: linkId, kind: 'fixes', to: bugId, created_by: actor, created_at: now });
	task.summary.fixes.push(bugId);
	bug.summary.fixed_by.push(task.summary.id);
	bug.summary.updated_at = now;
	if (!changed.includes(bug)) changed.push(bug);
}

/** Apply one call. Returns the new state and the answer; throws an `AppError` and changes nothing when refused. */
export function applyWork(state: WorkState, call: WorkCall, ctx: WorkContext): { state: WorkState; accepted: WorkAccepted } {
	const kept = typeof call.change_id === 'string' && Object.hasOwn(state.answers, call.change_id) ? state.answers[call.change_id] : undefined;
	if (kept) {
		if (kept.call !== JSON.stringify(call)) fail('invalid_request', 'That change id was used for a different change.');
		return { state, accepted: kept.accepted };
	}
	for (const [what, value] of [['change_id', call.change_id], ['item_id', 'item_id' in call ? call.item_id : undefined], ['step_id', 'step_id' in call ? call.step_id : undefined]] as const) {
		const needed = what === 'change_id' || (what === 'item_id' && 'item_id' in call) || (what === 'step_id' && 'step_id' in call);
		if (needed) idText(value, what);
	}
	if ('priority' in call && call.priority !== undefined && !['urgent', 'high', 'normal', 'low'].includes(call.priority)) fail('invalid_request', 'The priority must be urgent, high, normal or low.');
	if (call.method === 'task.set_status' && call.resolution !== undefined && !['completed', 'cancelled'].includes(call.resolution)) fail('invalid_request', 'A task resolution is completed or cancelled.');
	if ('done' in call && typeof call.done !== 'boolean') fail('invalid_request', 'done must be true or false.');
	const next: WorkState = structuredClone(state);
	const changed: StoredTask[] = [];
	const { now, actor } = ctx;

	switch (call.method) {
		case 'task.create': {
			const project = ctx.projects.find((p) => p.id === call.project_id) ?? fail('not_found', 'That project is not here.');
			if (project.archived) fail('invalid_request', 'That project is archived.');
			if (itemAt(next, call.item_id)) fail('already_exists', 'That item already exists.');
			const title = oneLine(call.title, WORK_LIMITS.title, 'The title');
			if (call.team_id && !project.teamIds.includes(call.team_id)) fail('invalid_request', "That team isn't one of the project's teams.");
			if (call.assignee_id && !ctx.members.includes(call.assignee_id)) fail('unknown_recipient', 'That member cannot be assigned.');
			const number = (next.next[call.project_id] ?? 0) + 1;
			next.next[call.project_id] = number;
			const checklist = call.checklist?.length ? steps(call.checklist, []) : [];
			const item: StoredTask = {
				summary: {
					id: call.item_id,
					kind: 'task',
					project_id: call.project_id,
					number,
					short_id: shortId(call.item_id),
					title,
					status: 'todo',
					priority: call.priority ?? 'normal',
					assignee_id: call.assignee_id,
					created_by: actor,
					created_at: now,
					updated_at: now,
					team_id: call.team_id,
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
				},
				description: call.description ? longText(call.description, WORK_LIMITS.description, 'The description') : undefined,
				definition_of_done: call.definition_of_done ? longText(call.definition_of_done, WORK_LIMITS.definitionOfDone, 'The definition of done') : undefined,
				checklist,
				watchers: [actor]
			};
			next.items[call.item_id] = item;
			next.seq += 1;
			changed.push(item);
			for (const f of call.fixes ?? []) {
				addFixLink(next, item, f.link_id, f.bug_id, changed, now, actor);
				followFixTasks(next, next.items[f.bug_id], now, changed, { kind: 'linked', taskStatus: item.summary.status });
			}
			break;
		}
		case 'task.update': {
			const item = findKind(next, call.item_id, 'task');
			expect(item, call.expected_version);
			const s = item.summary;
			let any = false;
			if (call.title !== undefined) ((s.title = oneLine(call.title, WORK_LIMITS.title, 'The title')), (any = true));
			if (call.description !== undefined && call.clear_description) fail('invalid_request', 'Give a description or clear it, not both.');
			if (call.description !== undefined) ((item.description = longText(call.description, WORK_LIMITS.description, 'The description')), (any = true));
			if (call.clear_description) ((item.description = undefined), (any = true));
			if (call.definition_of_done !== undefined && call.clear_definition_of_done) fail('invalid_request', 'Give a definition of done or clear it, not both.');
			if (call.definition_of_done !== undefined) ((item.definition_of_done = longText(call.definition_of_done, WORK_LIMITS.definitionOfDone, 'The definition of done')), (any = true));
			if (call.clear_definition_of_done) ((item.definition_of_done = undefined), (any = true));
			if (call.priority !== undefined) ((s.priority = call.priority), (any = true));
			if (call.team_id !== undefined && call.clear_team) fail('invalid_request', 'Give a team or clear it, not both.');
			if (call.team_id !== undefined) {
				const project = ctx.projects.find((p) => p.id === s.project_id);
				if (!project?.teamIds.includes(call.team_id)) fail('invalid_request', "That team isn't one of the project's teams.");
				((s.team_id = call.team_id), (any = true));
			}
			if (call.clear_team) ((s.team_id = undefined), (any = true));
			if (!any) fail('invalid_request', 'Nothing to change.');
			touch(next, item, now);
			changed.push(item);
			break;
		}
		case 'task.assign': {
			const item = findKind(next, call.item_id, 'task');
			expect(item, call.expected_version);
			if (call.assignee_id && !ctx.members.includes(call.assignee_id)) fail('unknown_recipient', 'That member cannot be assigned.');
			item.summary.assignee_id = call.assignee_id;
			touch(next, item, now);
			changed.push(item);
			break;
		}
		case 'task.set_status': {
			const item = findKind(next, call.item_id, 'task');
			expect(item, call.expected_version);
			const s = item.summary;
			const from = s.status;
			const to = call.status;
			const allowed: Record<string, string[]> = {
				todo: ['in_progress', 'blocked', 'done'],
				in_progress: ['todo', 'blocked', 'done'],
				blocked: ['todo', 'in_progress', 'done'],
				done: ['todo', 'in_progress']
			};
			if (!allowed[from]?.includes(to)) fail('invalid_request', `A task that is ${from.replace('_', ' ')} can't go to ${to.replace('_', ' ')}.`);
			if ((to === 'blocked') !== (call.blocked_reason !== undefined)) fail('invalid_request', to === 'blocked' ? 'Say what it is blocked on.' : 'A reason only goes with blocked.');
			if (call.blocked_reason !== undefined) s.blocked_reason = oneLine(call.blocked_reason, WORK_LIMITS.blockedReason, 'The reason');
			else s.blocked_reason = undefined;
			if (to !== 'done' && (call.resolution !== undefined || call.anyway_reason !== undefined)) fail('invalid_request', 'A resolution only goes with done.');
			if (to === 'done') {
				const resolution = call.resolution ?? 'completed';
				const left = item.checklist.filter((c) => c.required && !c.done);
				if (resolution === 'completed' && left.length > 0) {
					const anyway = call.anyway_reason;
					if (anyway === undefined) return fail('checklist_incomplete', `Required steps are not done: ${left.map((c) => c.text).join('; ')}.`);
					if (anyway.trim() === '') return fail('invalid_request', 'Say why it is done with required steps left.');
					item.done_anyway = { reason: longText(anyway.trim(), WORK_LIMITS.reason, 'The reason'), steps: left.map((c) => c.id) };
					s.done_anyway = true;
				}
				s.resolution = resolution;
				item.done_at = now;
			} else {
				item.done_at = undefined;
				if (to === 'in_progress' && !item.started_at) item.started_at = now;
				if (from === 'done') s.reopen_count += 1;
				s.resolution = undefined;
				item.done_anyway = undefined;
				s.done_anyway = false;
			}
			s.status = to;
			touch(next, item, now);
			changed.push(item);
			for (const bugId of s.fixes) followFixTasks(next, next.items[bugId], now, changed, { kind: 'task', from, to });
			break;
		}
		case 'bug.report': {
			const project = ctx.projects.find((p) => p.id === call.project_id) ?? fail('not_found', 'That project is not here.');
			if (project.archived) fail('invalid_request', 'That project is archived.');
			const title = oneLine(call.title, WORK_LIMITS.title, 'The title');
			const happened = bugText(call.what_happened, 'What happened', true);
			if (!SEVERITY_IDS.includes(call.severity)) fail('invalid_request', 'Pick a severity: critical, major, minor or trivial.');
			if (itemAt(next, call.item_id)) fail('already_exists', 'That item already exists.');
			const areaId = call.area_id ?? project.areas?.find((a) => a.general && !a.deleted)?.id;
			if (call.area_id && !project.areas?.some((a) => a.id === call.area_id && !a.deleted)) fail('invalid_request', "That area isn't one of the project's areas.");
			const key = `b:${call.project_id}`;
			const number = (next.next[key] ?? 0) + 1;
			next.next[key] = number;
			const checklist = call.checklist?.length ? steps(call.checklist, []) : [];
			const item: StoredTask = {
				summary: {
					id: call.item_id,
					kind: 'bug',
					project_id: call.project_id,
					number,
					short_id: shortId(call.item_id),
					title,
					status: 'new',
					priority: 'normal',
					created_by: actor,
					created_at: now,
					updated_at: now,
					area_id: areaId,
					severity: call.severity,
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
				},
				what_happened: happened,
				expected: call.expected ? bugText(call.expected, 'Expected', false) : undefined,
				steps: call.steps ? bugText(call.steps, 'Steps', false) : undefined,
				environment: call.environment ? longText(call.environment, 2000, 'The environment') : undefined,
				checklist,
				watchers: [actor]
			};
			next.items[call.item_id] = item;
			next.seq += 1;
			changed.push(item);
			break;
		}
		case 'bug.update': {
			const item = findKind(next, call.item_id, 'bug');
			expect(item, call.expected_version);
			const s = item.summary;
			const project = ctx.projects.find((p) => p.id === s.project_id);
			let any = false;
			if (call.title !== undefined) ((s.title = oneLine(call.title, WORK_LIMITS.title, 'The title')), (any = true));
			if (call.what_happened !== undefined) ((item.what_happened = bugText(call.what_happened, 'What happened', true)), (any = true));
			if (call.expected !== undefined && call.clear_expected) fail('invalid_request', 'Give a value or clear it, not both.');
			if (call.expected !== undefined) ((item.expected = bugText(call.expected, 'Expected', false)), (any = true));
			if (call.clear_expected) ((item.expected = undefined), (any = true));
			if (call.steps !== undefined && call.clear_steps) fail('invalid_request', 'Give a value or clear it, not both.');
			if (call.steps !== undefined) ((item.steps = bugText(call.steps, 'Steps', false)), (any = true));
			if (call.clear_steps) ((item.steps = undefined), (any = true));
			if (call.environment !== undefined && call.clear_environment) fail('invalid_request', 'Give a value or clear it, not both.');
			if (call.environment !== undefined) ((item.environment = longText(call.environment, 2000, 'The environment')), (any = true));
			if (call.clear_environment) ((item.environment = undefined), (any = true));
			if (call.severity !== undefined) {
				if (!SEVERITY_IDS.includes(call.severity)) fail('invalid_request', 'Pick a severity: critical, major, minor or trivial.');
				((s.severity = call.severity), (any = true));
			}
			if (call.priority !== undefined) ((s.priority = call.priority), (any = true));
			if (call.area_id !== undefined) {
				if (!project?.areas?.some((a) => a.id === call.area_id && !a.deleted)) fail('invalid_request', "That area isn't one of the project's areas.");
				((s.area_id = call.area_id), (any = true));
			}
			if (!any) fail('invalid_request', 'Nothing to change.');
			touch(next, item, now);
			changed.push(item);
			break;
		}
		case 'bug.triage': {
			const item = findKind(next, call.item_id, 'bug');
			expect(item, call.expected_version);
			const s = item.summary;
			if (s.status !== 'new') fail('invalid_request', `A bug that is ${s.status.replace('_', ' ')} is not waiting for triage.`);
			const project = ctx.projects.find((p) => p.id === s.project_id);
			if (!project?.areas?.some((a) => a.id === call.area_id && !a.deleted)) fail('invalid_request', "That area isn't one of the project's areas.");
			if (!SEVERITY_IDS.includes(call.severity)) fail('invalid_request', 'Pick a severity: critical, major, minor or trivial.');
			if (call.assignee_id && !ctx.members.includes(call.assignee_id)) fail('unknown_recipient', 'That member cannot be assigned.');
			s.area_id = call.area_id;
			s.severity = call.severity;
			s.severity_confirmed = true;
			s.priority = call.priority;
			s.assignee_id = call.assignee_id;
			s.status = 'triaged';
			touch(next, item, now);
			changed.push(item);
			followFixTasks(next, item, now, changed, { kind: 'triaged' });
			break;
		}
		case 'bug.assign': {
			const item = findKind(next, call.item_id, 'bug');
			expect(item, call.expected_version);
			if (call.assignee_id && !ctx.members.includes(call.assignee_id)) fail('unknown_recipient', 'That member cannot be assigned.');
			item.summary.assignee_id = call.assignee_id;
			touch(next, item, now);
			changed.push(item);
			break;
		}
		case 'bug.mark_fixed': {
			const item = findKind(next, call.item_id, 'bug');
			expect(item, call.expected_version);
			const s = item.summary;
			if (s.status !== 'triaged' && s.status !== 'in_progress') fail('invalid_request', `A bug that is ${s.status.replace('_', ' ')} can't be marked fixed.`);
			const note = longText(call.note.trim(), WORK_LIMITS.reason, 'The note');
			if (note === '') fail('invalid_request', 'Say what fixed it.');
			const left = item.checklist.filter((c) => c.required && !c.done);
			if (left.length > 0) {
				const anyway = call.anyway_reason;
				if (anyway === undefined) return fail('checklist_incomplete', `Required steps are not done: ${left.map((c) => c.text).join('; ')}.`);
				if (anyway.trim() === '') return fail('invalid_request', 'Say why it is fixed with required steps left.');
				item.done_anyway = { reason: longText(anyway.trim(), WORK_LIMITS.reason, 'The reason'), steps: left.map((c) => c.id) };
				s.done_anyway = true;
			}
			item.closed_reason = note;
			s.status = 'fixed';
			touch(next, item, now);
			changed.push(item);
			break;
		}
		case 'bug.verify': {
			const item = findKind(next, call.item_id, 'bug');
			expect(item, call.expected_version);
			const s = item.summary;
			if (s.status !== 'fixed') fail('invalid_request', 'Only a fixed bug can be verified.');
			if (call.outcome === 'fixed') {
				s.status = 'closed';
				s.resolution = 'verified';
				item.done_at = now;
			} else if (call.outcome === 'still_happens') {
				const seen = (call.note ?? '').trim();
				if (seen === '') fail('invalid_request', 'Say what you still see.');
				longText(seen, WORK_LIMITS.reason, 'The note');
				s.status = 'triaged';
				s.reopen_count += 1;
				s.done_anyway = false;
				item.done_anyway = undefined;
			} else fail('invalid_request', 'The outcome is fixed or still_happens.');
			touch(next, item, now);
			changed.push(item);
			break;
		}
		case 'bug.close': {
			const item = findKind(next, call.item_id, 'bug');
			expect(item, call.expected_version);
			const s = item.summary;
			if (!['new', 'triaged', 'in_progress', 'fixed'].includes(s.status)) fail('invalid_request', `A bug that is ${s.status.replace('_', ' ')} can't be closed.`);
			if (call.duplicate_of !== undefined) {
				const original = itemAt(next, call.duplicate_of);
				if (!original || original.summary.deleted || original.summary.kind !== 'bug') fail('not_found', 'That original bug is not here.');
				if (original.summary.id === s.id || original.summary.project_id !== s.project_id) fail('invalid_request', 'The original must be another bug in the same project.');
				if (original.summary.status === 'closed' && original.summary.resolution === 'duplicate') fail('invalid_request', 'That bug is itself closed as a duplicate.');
				s.duplicate_of = call.duplicate_of;
				s.resolution = 'duplicate';
				if (!original.watchers.includes(s.created_by)) original.watchers.push(s.created_by);
			} else {
				if (call.resolution !== 'wont_fix' && call.resolution !== 'cant_reproduce') fail('invalid_request', 'Close it as a duplicate, won\'t fix or can\'t reproduce.');
				const reason = longText((call.reason ?? '').trim(), WORK_LIMITS.reason, 'The reason');
				if (reason === '') fail('invalid_request', 'Give a reason.');
				item.closed_reason = reason;
				s.resolution = call.resolution;
			}
			s.status = 'closed';
			item.done_at = now;
			touch(next, item, now);
			changed.push(item);
			break;
		}
		case 'bug.reopen': {
			const item = findKind(next, call.item_id, 'bug');
			expect(item, call.expected_version);
			const s = item.summary;
			if (s.status !== 'closed') fail('invalid_request', 'Only a closed bug can be reopened.');
			if (call.note.trim() === '') fail('invalid_request', 'Say why it is reopened.');
			longText(call.note.trim(), WORK_LIMITS.reason, 'The note');
			s.status = s.resolution === 'duplicate' ? 'new' : 'triaged';
			s.reopen_count += 1;
			s.resolution = undefined;
			s.duplicate_of = undefined;
			item.done_at = undefined;
			touch(next, item, now);
			changed.push(item);
			break;
		}
		case 'request.create':
		case 'request.update':
		case 'request.route':
		case 'request.accept':
		case 'request.scope':
		case 'request.ask_changes':
		case 'request.approve':
		case 'request.decline':
		case 'request.merge':
			applyRequest(next, call, ctx, changed);
			break;
		case 'work.link': {
			if (call.kind !== 'fixes') fail('invalid_request', 'Only fix links are supported for now.');
			if (call.to.type !== 'item') return fail('invalid_request', 'A fix link points at a bug.');
			const bugId = call.to.item_id;
			const task = findKind(next, call.from_item_id, 'task');
			if (call.from_item_id === bugId) fail('invalid_request', 'An item cannot be linked to itself.');
			changed.push(task);
			addFixLink(next, task, call.link_id, bugId, changed, now, actor);
			touch(next, task, now);
			followFixTasks(next, next.items[bugId], now, changed, { kind: 'linked', taskStatus: task.summary.status });
			break;
		}
		case 'work.comment':
			applyComment(next, call, ctx, changed);
			break;
		case 'work.unlink': {
			const owner = Object.values(next.items).find((i) => i.links?.some((l) => l.id === call.link_id));
			if (!owner) fail('not_found', 'That link is not here.');
			const link = owner!.links!.find((l) => l.id === call.link_id)!;
			if (link.kind === 'from_request') fail('invalid_request', "A from_request link can't be removed.");
			owner!.links = owner!.links!.filter((l) => l.id !== call.link_id);
			owner!.summary.fixes = owner!.summary.fixes.filter((b) => b !== link.to);
			const bug = next.items[link.to];
			if (bug) bug.summary.fixed_by = bug.summary.fixed_by.filter((id) => id !== owner!.summary.id);
			// Unlinking changes no bug status (design §2.4).
			touch(next, owner!, now);
			changed.push(owner!);
			if (bug && !changed.includes(bug)) changed.push(bug);
			break;
		}
		case 'checklist.add': {
			const item = find(next, call.item_id);
			if (item.summary.kind === 'request') fail('invalid_request', "A request doesn't have a checklist.");
			const added = steps(call.steps, item.checklist);
			const at = call.after_step_id ? item.checklist.findIndex((c) => c.id === call.after_step_id) + 1 : item.checklist.length;
			if (call.after_step_id && at === 0) fail('not_found', 'That step is not here.');
			item.checklist.splice(at, 0, ...added);
			touch(next, item, now);
			changed.push(item);
			break;
		}
		case 'checklist.check': {
			const item = stepOwner(next, call.step_id);
			const step = item.checklist.find((c) => c.id === call.step_id)!;
			if (step.task_id) fail('invalid_request', 'This step follows its task.');
			if (step.done !== call.done) {
				step.done = call.done;
				step.version += 1;
				step.checked_by = call.done ? actor : undefined;
				step.checked_at = call.done ? now : undefined;
				touch(next, item, now);
			}
			changed.push(item);
			break;
		}
		case 'checklist.edit': {
			const item = stepOwner(next, call.step_id);
			const step = item.checklist.find((c) => c.id === call.step_id)!;
			if (step.version !== call.expected_version) fail('conflict', 'This step changed. Reload to see the latest.');
			if (call.text === undefined && call.required === undefined) fail('invalid_request', 'Nothing to change.');
			if (call.text !== undefined) step.text = oneLine(call.text, WORK_LIMITS.stepText, 'A step');
			if (call.required !== undefined) step.required = call.required;
			step.version += 1;
			touch(next, item, now);
			changed.push(item);
			break;
		}
		case 'checklist.remove': {
			const item = stepOwner(next, call.step_id);
			const step = item.checklist.find((c) => c.id === call.step_id)!;
			if (step.version !== call.expected_version) fail('conflict', 'This step changed. Reload to see the latest.');
			item.checklist = item.checklist.filter((c) => c.id !== call.step_id);
			touch(next, item, now);
			changed.push(item);
			break;
		}
		case 'work.delete': {
			const item = find(next, call.item_id);
			expect(item, call.expected_version);
			item.summary.deleted = true;
			touch(next, item, now);
			changed.push(item);
			break;
		}
	}

	const accepted: WorkAccepted = {
		status: 'committed',
		position: positionOf(next),
		items: changed.map((c) => structuredClone(c.summary))
	};
	next.answers[call.change_id] = { call: JSON.stringify(call), accepted };
	const ids = Object.keys(next.answers);
	if (ids.length > 1000) delete next.answers[ids[0]];
	return { state: next, accepted };
}

/** The statuses that end an item: a task's done, a bug's closed, a request's approved, declined or merged. */
const FINISHED: readonly string[] = ['done', 'closed', 'approved', 'declined', 'merged'];

export const positionOf = (s: WorkState): WorkPosition => ({ incarnation: s.incarnation, seq: s.seq });

/**
 * The tasks of a project (or all), not deleted. Done tasks are listed only when they went Done at or after
 * `closedSince` (as `work.snapshot { closed_since }` does), or all of them when it is not given.
 */
export const summaries = (s: WorkState, projectId?: string, closedSince?: string): WorkItemSummary[] =>
	Object.values(s.items)
		.filter((i) => !i.summary.deleted && (projectId === undefined || i.summary.project_id === projectId))
		.filter((i) => !FINISHED.includes(i.summary.status) || closedSince === undefined || (i.done_at ?? i.summary.updated_at) >= closedSince)
		.map((i) => i.summary)
		.sort((a, b) => (a.number ?? 0) - (b.number ?? 0));

const timeOf = (item: StoredTask, now: string): TimeFigures => ({
	// Lead time runs to Done and stops there; cycle time from the first start.
	lead_ms: Math.max(0, Date.parse(item.done_at ?? now) - Date.parse(item.summary.created_at)),
	cycle_ms: item.started_at ? Math.max(0, Date.parse(item.done_at ?? now) - Date.parse(item.started_at)) : undefined,
	waiting_on_owner_ms: 0,
	blocked_ms: 0,
	rest_ms: Math.max(0, Date.parse(item.done_at ?? now) - Date.parse(item.summary.created_at)),
	open: !FINISHED.includes(item.summary.status),
	rounds: 1 + item.summary.reopen_count,
	as_of: now
});

const linkOf = (owner: StoredTask, l: NonNullable<StoredTask['links']>[number], s: WorkState) => ({
	id: l.id,
	from_item_id: owner.summary.id,
	kind: l.kind,
	to: { type: 'item' as const, item_id: l.to },
	created_by: l.created_by,
	created_at: l.created_at,
	target_deleted: !!s.items[l.to]?.summary.deleted
});
/** The links an item makes, or (backlinks) the links that point at it. */
function linksOf(s: WorkState, id: string, back: boolean) {
	return Object.values(s.items).flatMap((owner) =>
		(owner.links ?? []).filter((l) => (back ? l.to === id : owner.summary.id === id)).map((l) => linkOf(owner, l, s))
	);
}

/** One task in full (`work.get`). */
export function itemOf(s: WorkState, id: string, now: string): WorkItem {
	const item = find(s, id);
	return structuredClone({
		summary: item.summary,
		backlinks_more: false,
		watchers_more: false,
		description: item.description,
		definition_of_done: item.definition_of_done,
		what_happened: item.what_happened,
		expected: item.expected,
		steps: item.steps,
		environment: item.environment,
		closed_reason: item.closed_reason,
		text: item.text,
		route_reason: item.route_reason,
		routed_by: item.routed_by,
		outcome: item.outcome,
		merged_into: item.merged_into,
		decline_reason: item.decline_reason,
		done_anyway_detail: item.done_anyway,
		checklist: item.checklist,
		links: linksOf(s, id, false),
		backlinks: linksOf(s, id, true),
		watchers: item.watchers,
		time: timeOf(item, now)
	});
}

const ENDED: readonly string[] = ['done', 'closed', 'approved', 'declined', 'merged'];
const LIST_TEXT_MAX = 200;
const LIST_LIMIT_MAX = 500;
const wordsIn = (text: string): string[] => text.toLowerCase().match(/[\p{L}\p{N}]+/gu) ?? [];

/**
 * `work.list`: the items that pass the filters, newest number first. `text` is case-insensitive and matches word
 * prefixes over the title and a request's own words: every word of it must start some word there.
 */
export function listItems(s: WorkState, q: WorkListQuery): WorkItemSummary[] {
	if (q.text !== undefined && (typeof q.text !== 'string' || [...q.text].length > LIST_TEXT_MAX)) fail('invalid_request', `text can be at most ${LIST_TEXT_MAX} characters.`);
	if (q.limit !== undefined && (!Number.isInteger(q.limit) || q.limit < 1 || q.limit > LIST_LIMIT_MAX)) fail('invalid_request', `limit is 1 to ${LIST_LIMIT_MAX}.`);
	const wanted = q.text === undefined ? [] : wordsIn(q.text);
	return Object.values(s.items)
		.filter((i) => !i.summary.deleted)
		.filter((i) => q.project_id === undefined || i.summary.project_id === q.project_id)
		.filter((i) => !q.kinds || (q.kinds as string[]).includes(i.summary.kind))
		.filter((i) => (q.statuses ? (q.statuses as string[]).includes(i.summary.status) : q.include_closed || !ENDED.includes(i.summary.status)))
		.filter((i) => {
			if (wanted.length === 0) return true;
			const have = wordsIn(`${i.summary.title} ${i.text ?? ''}`);
			return wanted.every((w) => have.some((h) => h.startsWith(w)));
		})
		.map((i) => i.summary)
		.sort((a, b) => (b.number ?? 0) - (a.number ?? 0))
		.slice(0, q.limit ?? 100);
}

/** The events of one request, oldest first (`work.activity`). Other kinds keep none yet. */
export function activityOf(s: WorkState, id: string): WorkEvent[] {
	return structuredClone(find(s, id).events ?? []);
}
