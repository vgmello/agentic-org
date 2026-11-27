import { toast } from 'svelte-sonner';
import { UNAVAILABLE_MESSAGE } from '$lib/platform/unavailable-api';
import { toAppError } from '$lib/proto';
import { workItemsApi } from '../../api';
import { taskHref, titleError } from '../../present';
import type { Choices, NewTask, Priority, Severity, TaskType } from '../../types';

export type ChoicesState = 'loading' | 'ready' | 'unavailable' | 'unsupported' | 'error';

export function classifyChoicesError(e: unknown): {
	state: 'unavailable' | 'unsupported' | 'error';
	message: string;
} {
	const failure = toAppError(e);
	if (
		failure.source === 'api' &&
		failure.code === 'unavailable' &&
		failure.message === UNAVAILABLE_MESSAGE
	) {
		return { state: 'unavailable', message: failure.message };
	}
	if (failure.source === 'api' && failure.code === 'unsupported') {
		return { state: 'unsupported', message: failure.message };
	}
	return { state: 'error', message: failure.message };
}

export function getBugSubmitDisabledReason(
	type: TaskType,
	title: string,
	whatHappened: string
): string | undefined {
	if (type !== 'bug') return undefined;
	const missingTitle = !title.trim();
	const missingWhat = !whatHappened.trim();
	if (missingTitle && missingWhat) return 'Fill in Title and What happened to report a bug.';
	if (missingTitle) return 'Fill in Title to report a bug.';
	if (missingWhat) return 'Fill in What happened to report a bug.';
	return undefined;
}

export function validateCaptureDraft(input: {
	type: TaskType;
	title: string;
	whatHappened: string;
	effectiveTeamId: string;
}): Record<string, string> {
	const nextErrors: Record<string, string> = {};
	const tErr = titleError(input.title);
	if (tErr) nextErrors.title = tErr;
	if (input.type === 'bug' && !input.whatHappened.trim()) {
		nextErrors.whatHappened = 'Describe what happened.';
	}
	if (!input.effectiveTeamId) {
		nextErrors.team = 'Choose an accountable team or select a project.';
	}
	return nextErrors;
}

export function buildCapturePayload(input: {
	operationId: string;
	type: TaskType;
	title: string;
	description: string;
	whatHappened: string;
	expected: string;
	steps: string;
	environment: string;
	chosenProjectId: string;
	effectiveTeamId: string;
	assignedTeamId: string;
	assigneeId: string;
	priority: Priority;
	severity: Severity | null;
	fixesTaskId?: string;
	relatesToTaskId?: string;
}): NewTask {
	const bugFields =
		input.type === 'bug'
			? {
					bug: {
						whatHappened: input.whatHappened.trim(),
						expected: input.expected.trim(),
						steps: input.steps.trim(),
						environment: input.environment.trim()
					}
				}
			: {};
	return {
		operationId: input.operationId,
		type: input.type,
		title: input.title.trim(),
		description: input.type === 'bug' ? input.whatHappened.trim() : input.description.trim(),
		projectId: input.chosenProjectId || null,
		teamId: input.effectiveTeamId,
		assignedTeamId: input.assignedTeamId || null,
		assigneeId: input.assigneeId || null,
		priority: input.priority,
		severity: input.type === 'bug' ? input.severity : null,
		...bugFields,
		...(input.fixesTaskId ? { fixesTaskId: input.fixesTaskId } : {}),
		...(input.relatesToTaskId ? { relatesToTaskId: input.relatesToTaskId } : {})
	};
}

export function formatCreatedToast(
	type: TaskType,
	ref: string,
	choices: Choices | null,
	assigneeId: string
): string {
	if (type === 'bug') return `Bug ${ref} saved.`;
	if (type === 'request') {
		const assigneeName = choices?.employees.find((e) => e.id === assigneeId)?.name;
		return assigneeName ? `${ref} sent to ${assigneeName}.` : `Request ${ref} saved.`;
	}
	return `Task ${ref} saved.`;
}

export function buildExistingCommentText(input: {
	type: TaskType;
	title: string;
	description: string;
	whatHappened: string;
	steps: string;
}): string {
	const parts =
		input.type === 'bug'
			? [input.title.trim(), input.whatHappened.trim(), input.steps.trim()]
			: [input.title.trim(), input.description.trim()];
	return parts.filter(Boolean).join('\n\n') || input.title.trim();
}

export async function persistDraftSteps(
	organizationId: string,
	taskId: string,
	pending: string[]
): Promise<void> {
	if (pending.length === 0) return;
	try {
		await workItemsApi.addSteps(organizationId, taskId, pending);
	} catch {
		toast.warning('Task created, but checklist steps could not be attached.');
	}
}

export async function runTessIntakeBackground(input: {
	organizationId: string;
	jobId: string;
	payload: NewTask;
	stepsToAttach: string[];
	capturedTitle: string;
	capturedType: TaskType;
	mode: 'draft' | 'direct';
	assigneeName?: string;
	finishJob?: (
		id: string,
		meta: {
			title: string;
			summary: string;
			href: string;
			icon: 'pencil' | 'check';
			tone: 'accent' | 'ok';
		}
	) => void;
	failJob?: (id: string, title: string, error: string, href: string) => void;
	oncreated?: (created: { id: string; ref: string; type: TaskType }) => void;
	onrefresh?: () => void;
}): Promise<void> {
	try {
		const created = await workItemsApi.create(input.organizationId, input.payload);
		await persistDraftSteps(input.organizationId, created.id, input.stepsToAttach);
		const isDraft = input.mode === 'draft';
		const summary = isDraft
			? `${input.capturedTitle} · Ready for Draft ideation`
			: `${input.capturedTitle}${input.assigneeName ? ` · Assigned to ${input.assigneeName}` : ''}`;
		input.finishJob?.(input.jobId, {
			title: isDraft ? `Tess drafted ${created.ref}` : `Tess filed ${created.ref}`,
			summary,
			href: taskHref(created.ref),
			icon: isDraft ? 'pencil' : 'check',
			tone: isDraft ? 'accent' : 'ok'
		});
		input.oncreated?.({ ...created, type: input.capturedType });
		input.onrefresh?.();
	} catch (e) {
		const msg = toAppError(e).message;
		input.failJob?.(input.jobId, `Couldn't file "${input.capturedTitle}"`, msg, '/work');
		toast.error(msg);
	}
}
