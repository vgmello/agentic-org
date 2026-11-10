import type { Severity } from '$lib/proto';

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

/**
 * `bug.from_sentence`: the fields of a bug report from one sentence. The draft is the fields of the
 * real `bug.report` call (`org-proto`), so it fills the report dialog as it is. The copilot never
 * reports the bug: the person confirms in the dialog.
 */
export interface BugFromSentenceInput {
	sentence: string;
}
export interface BugDraft {
	title: string;
	what_happened: string;
	severity: Exclude<Severity, 'unknown'>;
	/** One string, one step per line. */
	steps?: string;
	expected?: string;
	/** Only meaningful when it is the id of an area given as context. */
	area_id?: string;
	environment?: string;
}

/** A piece of the workspace the app hands the copilot as data (`copilot.draft`'s resolved `context_refs`). */
export interface ContextItem {
	kind: 'area';
	label: string;
	text: string;
}

/** The features built so far, with the shape of each one's input and draft. */
export interface FeatureShapes {
	'text.improve': { input: TextImproveInput; draft: TextDraft };
	'bug.from_sentence': { input: BugFromSentenceInput; draft: BugDraft };
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

/** What `copilot.*` can fail with. `invalid_request` and `too_large` already exist in `org-proto`; the rest are new. */
export const COPILOT_ERROR_CODES = [
	'copilot_unavailable',
	'copilot_failed',
	'timeout',
	'copilot_busy',
	'invalid_request',
	'too_large'
] as const;
export type CopilotErrorCode = (typeof COPILOT_ERROR_CODES)[number];
