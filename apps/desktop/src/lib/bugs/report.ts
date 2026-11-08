import { toAppError } from '$lib/office/api';
import type { AppError } from '$lib/proto';
import type { BugDraft, Severity } from '$lib/copilot/types';
import { SEVERITIES } from '$lib/copilot/types';

/** The bug report form as the person edits it: steps are one per line. */
export interface BugForm {
	title: string;
	actual: string;
	expected: string;
	steps: string;
	severity: Severity;
}

/** What gets reported. */
export interface BugReport {
	title: string;
	actual: string;
	expected: string;
	steps: string[];
	severity: Severity;
}

export const BUG_LIMITS = { title: 120, actual: 800, expected: 800, steps: 10, step: 300 } as const;

export const emptyForm = (): BugForm => ({ title: '', actual: '', expected: '', steps: '', severity: 'medium' });

export const isBlank = (f: BugForm) => !f.title.trim() && !f.actual.trim() && !f.expected.trim() && !f.steps.trim();

/** The copilot's draft, filling the normal form. Nothing is reported until the person confirms. */
export function formFromDraft(draft: BugDraft): BugForm {
	return {
		title: draft.title,
		actual: draft.actual,
		expected: draft.expected,
		steps: draft.steps.map((s) => s.replace(/\s*\n\s*/g, ' ')).join('\n'),
		severity: draft.severity
	};
}

export const stepLines = (steps: string): string[] =>
	steps
		.split('\n')
		.map((s) => s.trim())
		.filter(Boolean);

const chars = (s: string) => [...s].length;

export type FormErrors = Partial<Record<keyof BugForm, string>>;

/** Check the form with the report's rules (the copilot's draft has the same caps, and is made to fit the form). */
export function validateForm(form: BugForm): { ok: true; report: BugReport } | { ok: false; errors: FormErrors } {
	const errors: FormErrors = {};
	const title = form.title.trim();
	const actual = form.actual.trim();
	const expected = form.expected.trim();
	const steps = stepLines(form.steps);
	if (!title) errors.title = 'Give the bug a title.';
	else if (chars(title) > BUG_LIMITS.title) errors.title = `Keep the title under ${BUG_LIMITS.title} characters.`;
	if (!actual) errors.actual = 'Say what happens.';
	else if (chars(actual) > BUG_LIMITS.actual) errors.actual = `Keep it under ${BUG_LIMITS.actual} characters.`;
	if (chars(expected) > BUG_LIMITS.expected) errors.expected = `Keep it under ${BUG_LIMITS.expected} characters.`;
	if (steps.length > BUG_LIMITS.steps) errors.steps = `At most ${BUG_LIMITS.steps} steps.`;
	else if (steps.some((s) => chars(s) > BUG_LIMITS.step)) errors.steps = `Keep each step under ${BUG_LIMITS.step} characters.`;
	if (!SEVERITIES.includes(form.severity)) errors.severity = 'Pick a severity.';
	if (Object.keys(errors).length > 0) return { ok: false, errors };
	return { ok: true, report: { title, actual, expected, steps, severity: form.severity } };
}

/** Where reports go. TEMPORARY mock until work items exist in the office. */
export interface BugApi {
	/** True while reports go to the mock, which keeps them in memory only. */
	readonly sample: boolean;
	report(report: BugReport): Promise<{ id: string }>;
}

export function createMockBugs({ delayMs = 500 }: { delayMs?: number } = {}): BugApi {
	let count = 0;
	return {
		sample: true,
		async report() {
			await new Promise((resolve) => setTimeout(resolve, delayMs));
			return { id: `BUG-${++count}` };
		}
	};
}

export const bugApi: BugApi = createMockBugs();

export type SubmitResult =
	| { kind: 'invalid'; errors: FormErrors }
	| { kind: 'sent'; id: string; report: BugReport }
	| { kind: 'failed'; error: AppError };

/** Confirm: check the form, then report it. Never throws. */
export async function submitReport(api: BugApi, form: BugForm): Promise<SubmitResult> {
	const checked = validateForm(form);
	if (!checked.ok) return { kind: 'invalid', errors: checked.errors };
	try {
		const { id } = await api.report(checked.report);
		return { kind: 'sent', id, report: checked.report };
	} catch (e) {
		return { kind: 'failed', error: toAppError(e) };
	}
}
