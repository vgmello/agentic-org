// Rules every work call shares: the refusals, the text limits, the version bump and the checklist steps. Both the task
// and bug rules (engine.ts) and the request rules (requests.ts) build on these.
import type { ApiErrorCode, AppError, ChecklistStep, NewStep } from '$lib/proto';
import { WORK_LIMITS } from '$lib/proto';
import type { StoredTask, WorkState } from './engine';

export function fail(code: ApiErrorCode, message: string): never {
	const error: AppError = { source: 'api', code, message };
	throw error;
}

export const oneLine = (text: string | undefined, max: number, what: string): string => {
	const t = (text ?? '').trim();
	if (t === '' || /[\r\n]/.test(t)) fail('invalid_request', `${what} must be one line of text.`);
	if ([...t].length > max) fail('invalid_request', `${what} can be at most ${max} characters.`);
	return t;
};
export const longText = (text: string, max: number, what: string): string => {
	if ([...text].length > max) fail('invalid_request', `${what} can be at most ${max} characters.`);
	return text;
};

// The office's own text rules (org-work text.rs, design §2.1), which the request calls follow: trimmed; no control
// characters, line or paragraph separators or bidirectional formatting in a one-line field; a long text allows only a
// line feed and a tab (Windows line ends read as line feeds). Task and bug text keeps the looser rules above, which the
// Rust twin of this engine shares.
const BIDI = /[\u200e\u200f\u202a-\u202e\u2066-\u2069]/u;
const ONE_LINE_BAD = /[\p{Cc}\u2028\u2029]/u;
const LONG_BAD = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f]/u;

/** `str::trim` in Rust: strips Unicode White_Space (which has U+0085 and not the byte order mark), unlike JS `trim`. */
const rustTrim = (text: string): string => text.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, '');

export function officeLine(text: unknown, max: number, what: string): string {
	if (typeof text !== 'string') return fail('invalid_request', `${what} must be text.`);
	const t = rustTrim(text);
	if (t === '') return fail('invalid_request', `${what} can't be empty.`);
	if (ONE_LINE_BAD.test(t) || BIDI.test(t)) return fail('invalid_request', `${what} must be one line of plain text.`);
	if ([...t].length > max) return fail('invalid_request', `${what} can be at most ${max} characters.`);
	return t;
}
export function officeText(text: unknown, max: number, what: string): string {
	if (typeof text !== 'string') return fail('invalid_request', `${what} must be text.`);
	const t = rustTrim(text.replaceAll('\r\n', '\n'));
	if (t === '') return fail('invalid_request', `${what} can't be empty.`);
	if (LONG_BAD.test(t) || BIDI.test(t)) return fail('invalid_request', `${what} can't have control characters other than new lines and tabs.`);
	if ([...t].length > max) return fail('invalid_request', `${what} can be at most ${max} characters.`);
	return t;
}

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

/** `line` judges each step's text: the task rule by default, the office's one-line rule for a step an approval makes. */
export function steps(list: NewStep[], existing: ChecklistStep[], line: (text: string, max: number, what: string) => string = oneLine): ChecklistStep[] {
	// Only a list of the wrong shape is refused here (it used to crash); what the steps hold is judged as before.
	if (!Array.isArray(list) || !list.every((s) => typeof s === 'object' && s !== null && typeof s.step_id === 'string' && typeof s.text === 'string' && (s.required === undefined || typeof s.required === 'boolean'))) fail('invalid_request', 'A checklist is a list of steps, each with an id, text and required or not.');
	if (list.length < 1 || existing.length + list.length > WORK_LIMITS.maxSteps) fail('invalid_request', `A checklist can have at most ${WORK_LIMITS.maxSteps} steps.`);
	const seen = new Set(existing.map((s) => s.id));
	return list.map((s) => {
		if (seen.has(s.step_id)) fail('already_exists', 'That step is already there.');
		seen.add(s.step_id);
		return { id: s.step_id, text: line(s.text, WORK_LIMITS.stepText, 'A step'), required: s.required, done: false, version: 1 };
	});
}


/** The item stored under `id`: an own entry only, so an id such as `constructor` or `__proto__` finds nothing. */
export const itemAt = (state: WorkState, id: unknown): StoredTask | undefined => (typeof id === 'string' && Object.hasOwn(state.items, id) ? state.items[id] : undefined);

/** A client-made id: 1 to 128 characters, and not `__proto__` (which would rewrite an object's prototype as a key). */
export const idText = (value: unknown, what: string): string => {
	if (typeof value !== 'string' || value === '' || value.length > 128 || value === '__proto__') return fail('invalid_request', `${what} must be a text of 1 to 128 characters.`);
	return value;
};
