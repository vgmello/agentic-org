// The mock office's work-item rules for tasks and their checklists (ada's W7 design §1.10, §2, §4.4, §4.7), as pure
// functions over a plain state. It stands in for the office until `org_proto::work` exists; bugs, requests, links and
// comments come later. Errors are thrown as `AppError`s with the codes the design names.
import type {
	AppError,
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
	WorkItemSummary,
	WorkPosition
} from '$lib/proto';
import { WORK_LIMITS } from '$lib/proto';

export interface StoredTask {
	summary: WorkItemSummary;
	description?: string;
	definition_of_done?: string;
	done_anyway?: { reason: string; steps: string[] };
	/** When it first went In progress, and when it last went Done (a round closes at Done). */
	started_at?: string;
	done_at?: string;
	checklist: ChecklistStep[];
	watchers: string[];
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
	projects: { id: string; archived?: boolean; teamIds: string[] }[];
	/** Members who can be assigned (not offboarded). */
	members: string[];
}

export type WorkCall =
	| ({ method: 'task.create' } & TaskCreate)
	| ({ method: 'task.update' } & TaskUpdate)
	| ({ method: 'task.assign' } & TaskAssign)
	| ({ method: 'task.set_status' } & TaskSetStatus)
	| { method: 'checklist.add'; change_id: string; item_id: string; steps: NewStep[]; after_step_id?: string }
	| { method: 'checklist.check'; change_id: string; step_id: string; done: boolean }
	| { method: 'checklist.edit'; change_id: string; step_id: string; expected_version: number; text?: string; required?: boolean }
	| { method: 'checklist.remove'; change_id: string; step_id: string; expected_version: number }
	| { method: 'work.delete'; change_id: string; item_id: string; expected_version: number };

const fail = (code: ApiErrorCode, message: string): never => {
	const error: AppError = { source: 'api', code, message };
	throw error;
};

const oneLine = (text: string | undefined, max: number, what: string): string => {
	const t = (text ?? '').trim();
	if (t === '' || /[\r\n]/.test(t)) fail('invalid_request', `${what} must be one line of text.`);
	if ([...t].length > max) fail('invalid_request', `${what} can be at most ${max} characters.`);
	return t;
};
const longText = (text: string, max: number, what: string): string => {
	if ([...text].length > max) fail('invalid_request', `${what} can be at most ${max} characters.`);
	return text;
};

const shortId = (id: string) => id.slice(-6).toLowerCase();

export const emptyCounts = (steps: ChecklistStep[]) => ({
	total: steps.length,
	done: steps.filter((s) => s.done).length,
	required_left: steps.filter((s) => s.required && !s.done).length
});

function find(state: WorkState, id: string): StoredTask {
	const item = state.items[id];
	if (!item || item.summary.deleted) fail('not_found', 'That item is not here.');
	return item;
}
function stepOwner(state: WorkState, stepId: string): StoredTask {
	const owner = Object.values(state.items).find((i) => !i.summary.deleted && i.checklist.some((s) => s.id === stepId));
	return owner ?? fail('not_found', 'That checklist step is not here.');
}
const expect = (item: StoredTask, version: number) => {
	if (item.summary.version !== version) fail('conflict', 'This changed since you opened it. Reload to see the latest.');
};

function touch(state: WorkState, item: StoredTask, now: string) {
	item.summary.version += 1;
	item.summary.updated_at = now;
	item.summary.checklist = emptyCounts(item.checklist);
	state.seq += 1;
}

function steps(list: NewStep[], existing: ChecklistStep[]): ChecklistStep[] {
	if (list.length < 1 || existing.length + list.length > WORK_LIMITS.maxSteps) fail('invalid_request', `A checklist can have at most ${WORK_LIMITS.maxSteps} steps.`);
	const seen = new Set(existing.map((s) => s.id));
	return list.map((s) => {
		if (seen.has(s.step_id)) fail('already_exists', 'That step is already there.');
		seen.add(s.step_id);
		return { id: s.step_id, text: oneLine(s.text, WORK_LIMITS.stepText, 'A step'), required: s.required, done: false, version: 1 };
	});
}

/** Apply one call. Returns the new state and the answer; throws an `AppError` and changes nothing when refused. */
export function applyWork(state: WorkState, call: WorkCall, ctx: WorkContext): { state: WorkState; accepted: WorkAccepted } {
	const kept = state.answers[call.change_id];
	if (kept) {
		if (kept.call !== JSON.stringify(call)) fail('invalid_request', 'That change id was used for a different change.');
		return { state, accepted: kept.accepted };
	}
	const next: WorkState = structuredClone(state);
	const changed: StoredTask[] = [];
	const { now, actor } = ctx;

	switch (call.method) {
		case 'task.create': {
			const project = ctx.projects.find((p) => p.id === call.project_id) ?? fail('not_found', 'That project is not here.');
			if (project.archived) fail('invalid_request', 'That project is archived.');
			if (next.items[call.item_id]) fail('already_exists', 'That item already exists.');
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
			break;
		}
		case 'task.update': {
			const item = find(next, call.item_id);
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
			const item = find(next, call.item_id);
			expect(item, call.expected_version);
			if (call.assignee_id && !ctx.members.includes(call.assignee_id)) fail('unknown_recipient', 'That member cannot be assigned.');
			item.summary.assignee_id = call.assignee_id;
			touch(next, item, now);
			changed.push(item);
			break;
		}
		case 'task.set_status': {
			const item = find(next, call.item_id);
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
			break;
		}
		case 'checklist.add': {
			const item = find(next, call.item_id);
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

export const positionOf = (s: WorkState): WorkPosition => ({ incarnation: s.incarnation, seq: s.seq });

/**
 * The tasks of a project (or all), not deleted. Done tasks are listed only when they went Done at or after
 * `closedSince` (as `work.snapshot { closed_since }` does), or all of them when it is not given.
 */
export const summaries = (s: WorkState, projectId?: string, closedSince?: string): WorkItemSummary[] =>
	Object.values(s.items)
		.filter((i) => !i.summary.deleted && (projectId === undefined || i.summary.project_id === projectId))
		.filter((i) => i.summary.status !== 'done' || closedSince === undefined || (i.done_at ?? i.summary.updated_at) >= closedSince)
		.map((i) => i.summary)
		.sort((a, b) => (a.number ?? 0) - (b.number ?? 0));

const timeOf = (item: StoredTask, now: string): TimeFigures => ({
	// Lead time runs to Done and stops there; cycle time from the first start.
	lead_ms: Math.max(0, Date.parse(item.done_at ?? now) - Date.parse(item.summary.created_at)),
	cycle_ms: item.started_at ? Math.max(0, Date.parse(item.done_at ?? now) - Date.parse(item.started_at)) : undefined,
	waiting_on_owner_ms: 0,
	blocked_ms: 0,
	idle_ms: 0,
	open: item.summary.status !== 'done',
	rounds: 1 + item.summary.reopen_count,
	as_of: now
});

/** One task in full (`work.get`). */
export function itemOf(s: WorkState, id: string, now: string): WorkItem {
	const item = find(s, id);
	return structuredClone({
		summary: item.summary,
		backlinks_more: false,
		watchers_more: false,
		description: item.description,
		definition_of_done: item.definition_of_done,
		done_anyway_detail: item.done_anyway,
		checklist: item.checklist,
		links: [],
		backlinks: [],
		watchers: item.watchers,
		time: timeOf(item, now)
	});
}
