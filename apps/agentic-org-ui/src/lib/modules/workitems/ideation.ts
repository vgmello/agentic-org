// Draft Ideation lifecycle & clean execution-comment cutoff helpers (TASK-142).
// Draft items carry a leading `[Draft · Ideation]` marker until deterministically published to To do,
// and comments recorded during Draft ideation carry an `[ideation] ` prefix so execution views fold
// them out of the default agent-visible stream without requiring destructive edits.
import type { Comment, TaskDetail } from './types';

export const DRAFT_IDEATION_MARKER = '[Draft · Ideation]';
export const IDEATION_COMMENT_PREFIX = '[ideation] ';

const IDEATION_PREFIX_RE = /^\s*\[ideation\]\s*/i;

export function isDraftIdeationTask(task: Pick<TaskDetail, 'status' | 'description'>): boolean {
	return task.status === 'todo' && task.description.trimStart().startsWith(DRAFT_IDEATION_MARKER);
}

export function stripDraftIdeationMarker(description: string): string {
	const trimmed = description.trimStart();
	if (!trimmed.startsWith(DRAFT_IDEATION_MARKER)) return description;
	return trimmed.slice(DRAFT_IDEATION_MARKER.length).replace(/^\r?\n+/, '');
}

export function withDraftIdeationMarker(description: string): string {
	const cleaned = stripDraftIdeationMarker(description).trim();
	return cleaned ? `${DRAFT_IDEATION_MARKER}\n\n${cleaned}` : DRAFT_IDEATION_MARKER;
}

export function isIdeationComment(comment: Pick<Comment, 'body'>): boolean {
	return IDEATION_PREFIX_RE.test(comment.body);
}

export function formatIdeationCommentBody(body: string): string {
	const trimmed = body.trim();
	if (!trimmed) return '';
	return IDEATION_PREFIX_RE.test(trimmed) ? trimmed : `${IDEATION_COMMENT_PREFIX}${trimmed}`;
}

export function cleanCommentDisplayBody(body: string): string {
	return body.replace(IDEATION_PREFIX_RE, '');
}

export function splitIdeationComments(comments: readonly Comment[]): {
	ideation: Comment[];
	execution: Comment[];
} {
	const ideation: Comment[] = [];
	const execution: Comment[] = [];
	for (const comment of comments) {
		if (isIdeationComment(comment)) ideation.push(comment);
		else execution.push(comment);
	}
	return { ideation, execution };
}
