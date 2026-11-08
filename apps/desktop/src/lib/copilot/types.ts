// The copilot's calls, as the app sees them (`copilot.features` and `copilot.draft`).
// Mirrors `crates/org-copilot` and `docs/design/smart-features.md`. These are NOT in `org-proto`
// yet: they move into `proto.ts` when the office wires the calls and `org-proto` gains them.

/** One entry of `copilot.features`: what the app may put a smart button on. Empty means no runtime. */
export interface CopilotFeature {
	id: string;
	version: number;
	title: string;
}

/** `text.improve`: draft or improve any text. */
export interface TextImproveInput {
	mode: 'draft' | 'improve';
	/** The label of the field being written ("What happened"). */
	field: string;
	text: string;
	instruction?: string;
}
export interface TextDraft {
	text: string;
}

/** `bug.report`: a bug report from a sentence. */
export interface BugReportInput {
	sentence: string;
}
export type Severity = 'low' | 'medium' | 'high' | 'critical';
export const SEVERITIES: readonly Severity[] = ['low', 'medium', 'high', 'critical'];
export interface BugDraft {
	title: string;
	actual: string;
	expected: string;
	steps: string[];
	severity: Severity;
}

/** The features built so far, with the shape of each one's input and draft. */
export interface FeatureShapes {
	'text.improve': { input: TextImproveInput; draft: TextDraft };
	'bug.report': { input: BugReportInput; draft: BugDraft };
}
export type FeatureId = keyof FeatureShapes;

/** `copilot.draft`'s answer. The copilot only drafts: nothing has been saved or sent. */
export interface DraftResponse<D> {
	draft: D;
	/** Short remarks from the copilot (assumptions, gaps). */
	notes: string[];
}

/** The caps the office enforces (`crates/org-copilot`, `builtin.rs`). The app only pre-checks. */
export const COPILOT_LIMITS = {
	textMaxChars: 8000,
	fieldMaxChars: 80,
	instructionMaxChars: 500,
	sentenceMaxChars: 1000
} as const;

/** What `copilot.*` can fail with. `invalid_request` already exists in `org-proto`; the rest are new. */
export const COPILOT_ERROR_CODES = [
	'copilot_unavailable',
	'copilot_failed',
	'timeout',
	'copilot_busy',
	'invalid_request'
] as const;
export type CopilotErrorCode = (typeof COPILOT_ERROR_CODES)[number];
