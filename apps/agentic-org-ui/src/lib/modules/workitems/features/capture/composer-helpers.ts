import type { EmployeeChoice } from '../../types';

export interface MentionMatch {
	start: number;
	end: number;
	query: string;
}

/** Detects an active `@query` mention token ending at the caret position. */
export function detectMentionMatch(text: string, caret: number): MentionMatch | null {
	const before = text.slice(0, caret);
	const match = /(^|\s)@([a-zA-Z0-9._-]*)$/.exec(before);
	if (!match) return null;
	const query = match[2];
	return {
		start: caret - query.length - 1,
		end: caret,
		query
	};
}

/** Filters active employees matching an `@` mention query (up to 6 candidates). */
export function filterMentionCandidates(
	employees: readonly EmployeeChoice[],
	query: string
): EmployeeChoice[] {
	const q = query.trim().toLowerCase();
	return employees
		.filter((emp) => {
			if (!q) return true;
			return (
				emp.name.toLowerCase().includes(q) ||
				Boolean(emp.role?.toLowerCase().includes(q)) ||
				emp.kind.toLowerCase().includes(q)
			);
		})
		.slice(0, 6);
}

/** Replaces the active `@query` range with `@FirstName ` and returns the updated text + next caret index. */
export function applyMentionReplacement(
	text: string,
	match: MentionMatch,
	employee: EmployeeChoice
): { text: string; caret: number } {
	const first = employee.name.trim().split(/\s+/)[0] || employee.name;
	const inserted = `@${first} `;
	const nextText = `${text.slice(0, match.start)}${inserted}${text.slice(match.end)}`;
	return { text: nextText, caret: match.start + inserted.length };
}

/** Builds a clean Tess handoff prompt from whatever the user has typed so far. */
export function buildTessDraftPrompt(title: string, description: string, steps: string[]): string {
	const parts: string[] = [];
	const t = title.trim();
	const d = description.trim();
	if (t) parts.push(`Help me scope this task: ${t}`);
	if (d) parts.push(d);
	if (steps.length > 0) {
		parts.push(`Acceptance criteria:\n${steps.map((s) => `- ${s}`).join('\n')}`);
	}
	return parts.join('\n\n') || 'Help me scope a new task for this organization.';
}

export interface TessIntakeRouting {
	mode: 'draft' | 'direct';
	label: string;
}

const DRAFT_SIGNALS = /\b(explore|scope|design|draft|options|rfc|proposal|tbd|ideate)\b|\?/i;

/**
 * Predicts whether Tess will file a straight To-do task or start a Draft ideation lifecycle
 * based on scope complexity, open questions, or unassigned multi-part descriptions (TASK-139 / TASK-142).
 */
export function assessTessIntakeRouting(
	title: string,
	description: string,
	steps: readonly string[],
	assigneeName?: string
): TessIntakeRouting {
	const combined = `${title} ${description}`.trim();
	const looksExploratory =
		DRAFT_SIGNALS.test(combined) || description.trim().length >= 160 || steps.length >= 4;
	if (looksExploratory) {
		return {
			mode: 'draft',
			label: 'Tess will file as Draft for ideation & scoping'
		};
	}
	return {
		mode: 'direct',
		label: assigneeName
			? `Tess will refine & assign to ${assigneeName}`
			: 'Tess will refine & file to To do'
	};
}
