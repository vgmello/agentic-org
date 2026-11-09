// Rules every work call shares: the refusals, the text limits, the version bump and the checklist steps. Both the task
// and bug rules (engine.ts) and the request rules (requests.ts) build on these.
import type { ApiErrorCode, AppError, ChecklistStep, NewStep } from '$lib/proto';
import { WORK_LIMITS } from '$lib/proto';
import type { StoredTask, WorkState } from './engine';

export function fail(code: ApiErrorCode, message: string): never {
	const error: AppError = { source: 'api', code, message };
	throw error;
}

// Control characters (design §2.3): none in a one-line field; a long text allows only a line feed and a tab.
const CONTROL = /[\u0000-\u001f\u007f]/;
const LONG_CONTROL = /[\u0000-\u0008\u000b-\u001f\u007f]/;

export const oneLine = (text: unknown, max: number, what: string): string => {
	if (text !== undefined && typeof text !== 'string') return fail('invalid_request', `${what} must be text.`);
	const t = (text ?? '').trim();
	if (t === '' || CONTROL.test(t)) fail('invalid_request', `${what} must be one line of text.`);
	if ([...t].length > max) fail('invalid_request', `${what} can be at most ${max} characters.`);
	return t;
};
export const longText = (text: unknown, max: number, what: string): string => {
	if (typeof text !== 'string') return fail('invalid_request', `${what} must be text.`);
	if (LONG_CONTROL.test(text)) fail('invalid_request', `${what} can't have control characters.`);
	if ([...text].length > max) fail('invalid_request', `${what} can be at most ${max} characters.`);
	return text;
};

export const shortId = (id: string) => id.slice(-6).toLowerCase();

export const emptyCounts = (steps: ChecklistStep[]) => ({
	total: steps.length,
	done: steps.filter((s) => s.done).length,
	required_left: steps.filter((s) => s.required && !s.done).length
});

export function touch(state: WorkState, item: StoredTask, now: string) {
	item.summary.version += 1;
	item.summary.updated_at = now;
	item.summary.checklist = emptyCounts(item.checklist);
	state.seq += 1;
}

export function steps(list: NewStep[], existing: ChecklistStep[]): ChecklistStep[] {
	if (list.length < 1 || existing.length + list.length > WORK_LIMITS.maxSteps) fail('invalid_request', `A checklist can have at most ${WORK_LIMITS.maxSteps} steps.`);
	const seen = new Set(existing.map((s) => s.id));
	return list.map((s) => {
		if (seen.has(s.step_id)) fail('already_exists', 'That step is already there.');
		seen.add(s.step_id);
		return { id: s.step_id, text: oneLine(s.text, WORK_LIMITS.stepText, 'A step'), required: s.required, done: false, version: 1 };
	});
}


/** The item stored under `id`: an own entry only, so an id such as `constructor` or `__proto__` finds nothing. */
export const itemAt = (state: WorkState, id: unknown): StoredTask | undefined => (typeof id === 'string' && Object.hasOwn(state.items, id) ? state.items[id] : undefined);

/** A client-made id: 1 to 128 characters, and not `__proto__` (which would rewrite an object's prototype as a key). */
export const idText = (value: unknown, what: string): string => {
	if (typeof value !== 'string' || value === '' || value.length > 128 || value === '__proto__') return fail('invalid_request', `${what} must be a text of 1 to 128 characters.`);
	return value;
};
