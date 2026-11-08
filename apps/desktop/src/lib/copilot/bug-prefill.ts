import { fitTo } from './check';
import type { BugDraft } from './types';

/** The Report a bug dialog's fields, as the person edits them. `severity` and `area` are '' until chosen. */
export interface BugFields {
	title: string;
	happened: string;
	severity: string;
	area: string;
	expected: string;
	steps: string;
	environment: string;
}

/** The most characters a bug's environment may have (mirrors `WORK_ENVIRONMENT_MAX_CHARS` in `org-proto`). */
export const BUG_ENVIRONMENT_MAX_CHARS = 2000;

/** The longest each text field may be (the office's own caps; the dialog only pre-checks). */
export interface BugFieldLimits {
	title: number;
	text: number;
	environment: number;
}

/**
 * The dialog's fields with the copilot's draft filled in. The title, what happened and the severity
 * always come from the draft. The rest only when the draft has them: a field the draft leaves out
 * keeps what the person (or the app) put there. An `area_id` that isn't one of the project's areas
 * is ignored. Nothing is reported: the person still presses Report bug.
 */
export function prefillFromDraft(current: BugFields, draft: BugDraft, areas: { id: string }[], limits: BugFieldLimits): BugFields {
	const keep = (value: string | undefined, max: number, old: string) => (value?.trim() ? fitTo(value, max).text : old);
	return {
		title: fitTo(draft.title, limits.title).text,
		happened: fitTo(draft.what_happened, limits.text).text,
		severity: draft.severity,
		area: draft.area_id && areas.some((a) => a.id === draft.area_id) ? draft.area_id : current.area,
		expected: keep(draft.expected, limits.text, current.expected),
		steps: keep(draft.steps, limits.text, current.steps),
		environment: keep(draft.environment, limits.environment, current.environment)
	};
}

/** The names of the fields whose draft was longer than the field allows, so the person is told it was cut. */
export function clippedFields(draft: BugDraft, limits: BugFieldLimits): string[] {
	const over = (value: string | undefined, max: number) => (value?.trim() ? fitTo(value, max).clipped : false);
	return [
		over(draft.title, limits.title) && 'Title',
		over(draft.what_happened, limits.text) && 'What happened',
		over(draft.expected, limits.text) && 'Expected',
		over(draft.steps, limits.text) && 'Steps to reproduce',
		over(draft.environment, limits.environment) && 'Environment'
	].filter((x): x is string => typeof x === 'string');
}

/**
 * Fill the fields from a draft. `before` is what the person had before the FIRST draft: a second
 * draft keeps it, so Undo always brings back the person's own text and never an earlier draft.
 */
export function applyDraft(
	before: BugFields | undefined,
	current: BugFields,
	draft: BugDraft,
	areas: { id: string }[],
	limits: BugFieldLimits
): { fields: BugFields; before: BugFields; clipped: string[] } {
	return { fields: prefillFromDraft(current, draft, areas, limits), before: before ?? { ...current }, clipped: clippedFields(draft, limits) };
}

/** Undo: the person's own fields from before the first draft, or the fields as they are when there was no draft. */
export const undoDraft = (before: BugFields | undefined, current: BugFields): BugFields => (before ? { ...before } : current);
