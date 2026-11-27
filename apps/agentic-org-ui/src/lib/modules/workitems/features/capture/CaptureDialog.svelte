<!--
  @component
  Capture dialog for New task, Report a bug, New request, Create fix task and top-bar New › Work item
  (work-items-ux §4, §9; feature-requests-ux §1; BUG-2..9, TSK-5, RQS-1..3, RQS-13, RQS-17).
  Derives accountable team from chosen project or requires choosing one; assignment pair has independent
  Assigned team and Assignee; checks similar bugs (after 3 chars) and similar requests; guards dirty close.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import Plus from '@lucide/svelte/icons/plus';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import X from '@lucide/svelte/icons/x';
	import { newId } from '$lib/modules/organization/commands';
	import { useNotifications } from '$lib/platform/shell/notifications.svelte';
	import { useShellAi } from '$lib/platform/shell/shell-ai';
	import { useShellData } from '$lib/platform/shell/shell-data.svelte';
	import { UNAVAILABLE_MESSAGE } from '$lib/platform/unavailable-api';
	import { toAppError } from '$lib/proto';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import {
		errorClass,
		helpClass,
		inputClass,
		mobileButton,
		selectClass,
		textareaClass
	} from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import { OUTCOME_UNKNOWN } from '../../budget';
	import { withDraftIdeationMarker } from '../../ideation';
	import {
		PRIORITIES,
		PRIORITY_WORD,
		SEVERITIES,
		SEVERITY_HELP,
		SEVERITY_WORD,
		TYPE_WORD,
		splitSteps,
		stepError,
		taskHref,
		titleError
	} from '../../present';
	import type { Choices, EmployeeChoice, Priority, Severity, TaskRow, TaskType } from '../../types';
	import {
		applyMentionReplacement,
		assessTessIntakeRouting,
		buildTessDraftPrompt,
		detectMentionMatch,
		filterMentionCandidates,
		type MentionMatch
	} from './composer-helpers';

	let {
		open = $bindable(false),
		organizationId,
		initialType = 'task',
		allowTypeSwitch = false,
		projectId = null,
		fixesTaskId,
		relatesToTaskId,
		contextLabel,
		oncreated,
		onadded
	}: {
		open: boolean;
		organizationId: string;
		initialType?: TaskType;
		/** True when opened from top-bar New › Work item so the user can choose Task, Bug or Request. */
		allowTypeSwitch?: boolean;
		/** Pre-selected project when opened inside a project page. */
		projectId?: string | null;
		fixesTaskId?: string;
		relatesToTaskId?: string;
		contextLabel?: string;
		oncreated?: (created: { id: string; ref: string; type: TaskType }) => void;
		onadded?: (ref: string) => void;
	} = $props();

	const uid = $props.id();
	const shellAi = useShellAi();
	const notifications = useNotifications();
	let shellData: ReturnType<typeof useShellData> | undefined;
	try {
		shellData = useShellData();
	} catch {
		shellData = undefined;
	}
	let type = $state<TaskType>('task');
	let title = $state('');
	let description = $state('');
	let whatHappened = $state('');
	let expected = $state('');
	let steps = $state('');
	let environment = $state('');
	let severity = $state<Severity | null>(null);
	let priority = $state<Priority>('normal');
	let chosenProjectId = $state('');
	let chosenTeamId = $state('');
	let assignedTeamId = $state('');
	let assigneeId = $state('');
	let createMore = $state(false);
	let draftSteps = $state<string[]>([]);
	let stepInput = $state('');
	let stepInputError = $state<string | undefined>();
	let mentionedIds = $state<string[]>([]);
	let mentionMatch = $state<MentionMatch | null>(null);
	let mentionIdx = $state(0);

	type ChoicesState = 'loading' | 'ready' | 'unavailable' | 'unsupported' | 'error';

	let choices = $state.raw<Choices | null>(null);
	let choicesState = $state<ChoicesState>('loading');
	let choicesMessage = $state('');
	const choicesUnavailable = $derived(choicesState !== 'ready');
	let similar = $state.raw<TaskRow[]>([]);
	let similarTimer: ReturnType<typeof setTimeout> | undefined;

	let busy = $state(false);
	let checking = $state(false);
	let addingId = $state<string | null>(null);
	let operationId = $state('');
	let unknownOpId = $state<string | undefined>();
	let discardOpen = $state(false);
	let fieldErrors = $state<Record<string, string | undefined>>({});
	let formError = $state<string | undefined>();

	let titleInput = $state<HTMLInputElement | null>(null);
	let descriptionInput = $state<HTMLTextAreaElement | null>(null);
	let whatHappenedInput = $state<HTMLTextAreaElement | null>(null);
	let teamSelect = $state<HTMLSelectElement | null>(null);

	const mentionCandidates = $derived(
		mentionMatch && choices ? filterMentionCandidates(choices.employees, mentionMatch.query) : []
	);
	const mentionedEmployees = $derived(
		mentionedIds
			.map((id) => choices?.employees.find((e) => e.id === id))
			.filter((e): e is EmployeeChoice => Boolean(e))
	);

	const selectedProject = $derived(choices?.projects.find((p) => p.id === chosenProjectId) ?? null);
	const effectiveTeamId = $derived(selectedProject ? selectedProject.ownerTeam.id : chosenTeamId);
	const effectiveTeamName = $derived(
		selectedProject
			? selectedProject.ownerTeam.name
			: (choices?.teams.find((t) => t.id === chosenTeamId)?.name ?? '')
	);

	const dialogHeading = $derived.by(() => {
		const org = choices?.organizationName ? ` · ${choices.organizationName}` : '';
		if (fixesTaskId) return `Create fix task${org}`;
		if (type === 'bug') return `Report a bug${org}`;
		if (type === 'request') return `New request${org}`;
		return `New task${org}`;
	});

	const isDirty = $derived(
		title.trim().length > 0 ||
			description.trim().length > 0 ||
			whatHappened.trim().length > 0 ||
			expected.trim().length > 0 ||
			steps.trim().length > 0 ||
			draftSteps.length > 0
	);

	const bugSubmitDisabledReason = $derived.by(() => {
		if (type !== 'bug') return undefined;
		const missingTitle = !title.trim();
		const missingWhat = !whatHappened.trim();
		if (missingTitle && missingWhat) return 'Fill in Title and What happened to report a bug.';
		if (missingTitle) return 'Fill in Title to report a bug.';
		if (missingWhat) return 'Fill in What happened to report a bug.';
		return undefined;
	});

	const submitLabel = $derived.by(() => {
		if (type === 'bug') return busy ? 'Reporting…' : 'Report bug';
		if (type === 'request') return busy ? 'Saving…' : assigneeId ? 'Send request' : 'Save request';
		return busy ? 'Creating…' : 'Create task';
	});

	const selectedAssigneeName = $derived(choices?.employees.find((e) => e.id === assigneeId)?.name);

	const tessRouting = $derived(
		type !== 'bug' && title.trim()
			? assessTessIntakeRouting(title, description, draftSteps, selectedAssigneeName)
			: null
	);

	function classifyChoicesError(e: unknown): {
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

	async function loadChoices() {
		choicesState = 'loading';
		choicesMessage = '';
		try {
			const loaded = await workItemsApi.choices(organizationId);
			choices = loaded;
			choicesState = 'ready';
			if (projectId && loaded.projects.some((p) => p.id === projectId)) {
				chosenProjectId = projectId;
			} else if (!chosenProjectId && !chosenTeamId && loaded.teams.length === 1) {
				chosenTeamId = loaded.teams[0].id;
			}
		} catch (e) {
			const classified = classifyChoicesError(e);
			choices = null;
			choicesState = classified.state;
			choicesMessage = classified.message;
		}
	}

	function resetDraftFields(preserveScope = false) {
		title = '';
		description = '';
		whatHappened = '';
		expected = '';
		steps = '';
		environment = '';
		severity = null;
		priority = 'normal';
		if (!preserveScope) {
			chosenProjectId = projectId ?? '';
			chosenTeamId = '';
			assignedTeamId = '';
			assigneeId = '';
		}
		draftSteps = [];
		stepInput = '';
		stepInputError = undefined;
		mentionedIds = [];
		mentionMatch = null;
		mentionIdx = 0;
		similar = [];
		fieldErrors = {};
		formError = undefined;
		operationId = newId();
	}

	$effect(() => {
		if (!open) return;
		if (unknownOpId) return;
		type = initialType;
		createMore = false;
		resetDraftFields(false);
		void loadChoices();
		void tick().then(() => titleInput?.focus());
	});

	// Similar open bugs (after 3 title characters, up to 3) and similar open requests (up to 2).
	$effect(() => {
		if (!open || (type !== 'bug' && type !== 'request')) {
			similar = [];
			return;
		}
		const text = type === 'bug' ? title.trim() : `${title} ${description}`.trim();
		if (text.length < 3) {
			similar = [];
			return;
		}
		const targetType = type;
		const pId = chosenProjectId || null;
		const tId = effectiveTeamId || null;
		clearTimeout(similarTimer);
		similarTimer = setTimeout(async () => {
			try {
				similar = await workItemsApi.similar(organizationId, {
					type: targetType,
					text,
					projectId: pId,
					teamId: tId
				});
			} catch {
				similar = [];
			}
		}, 180);
		return () => clearTimeout(similarTimer);
	});

	function handleOpenChange(next: boolean) {
		if (next) {
			open = true;
			return;
		}
		if (busy || checking) {
			open = true;
			return;
		}
		if (isDirty && !unknownOpId) {
			open = true;
			discardOpen = true;
			return;
		}
		open = false;
	}

	async function confirmDiscard() {
		discardOpen = false;
		unknownOpId = undefined;
		open = false;
	}

	function discardAttempt() {
		if (busy || checking) return;
		unknownOpId = undefined;
		formError = undefined;
		operationId = newId();
		open = false;
	}

	async function focusField(field: string) {
		await tick();
		if (field === 'title') titleInput?.focus();
		else if (field === 'whatHappened') whatHappenedInput?.focus();
		else if (field === 'team') teamSelect?.focus();
	}

	function validateDraft(): Record<string, string> {
		const nextErrors: Record<string, string> = {};
		const tErr = titleError(title);
		if (tErr) nextErrors.title = tErr;
		if (type === 'bug' && !whatHappened.trim()) nextErrors.whatHappened = 'Describe what happened.';
		if (!effectiveTeamId) nextErrors.team = 'Choose an accountable team or select a project.';
		return nextErrors;
	}

	function buildCreatePayload(op: string) {
		const bugFields =
			type === 'bug'
				? {
						bug: {
							whatHappened: whatHappened.trim(),
							expected: expected.trim(),
							steps: steps.trim(),
							environment: environment.trim()
						}
					}
				: {};
		return {
			operationId: op,
			type,
			title: title.trim(),
			description: type === 'bug' ? whatHappened.trim() : description.trim(),
			projectId: chosenProjectId || null,
			teamId: effectiveTeamId,
			assignedTeamId: assignedTeamId || null,
			assigneeId: assigneeId || null,
			priority,
			severity: type === 'bug' ? severity : null,
			...bugFields,
			...(fixesTaskId ? { fixesTaskId } : {}),
			...(relatesToTaskId ? { relatesToTaskId } : {})
		};
	}

	function toastCreated(ref: string) {
		if (type === 'bug') {
			toast.success(`Bug ${ref} saved.`);
			return;
		}
		if (type === 'request') {
			const assigneeName = choices?.employees.find((e) => e.id === assigneeId)?.name;
			toast.success(assigneeName ? `${ref} sent to ${assigneeName}.` : `Request ${ref} saved.`);
			return;
		}
		toast.success(`Task ${ref} saved.`);
	}

	async function handleCreateError(e: unknown, op: string) {
		const err = toAppError(e);
		const raw = e as { code?: string; field?: string };
		if (raw?.code === OUTCOME_UNKNOWN) {
			unknownOpId = op;
			formError = "Save not confirmed (Unsaved). Check status so we don't create a duplicate item.";
			return;
		}
		operationId = newId();
		if (raw?.field) {
			fieldErrors = { [raw.field]: err.message };
			await focusField(raw.field);
		} else {
			formError = err.message;
		}
	}

	function handleDescriptionInput(event: Event) {
		const el = event.currentTarget as HTMLTextAreaElement;
		description = el.value;
		const nextMatch = detectMentionMatch(el.value, el.selectionStart ?? el.value.length);
		mentionMatch = nextMatch;
		mentionIdx = 0;
	}

	function pickMention(emp: EmployeeChoice) {
		if (!mentionMatch) return;
		const applied = applyMentionReplacement(description, mentionMatch, emp);
		description = applied.text;
		if (!mentionedIds.includes(emp.id)) mentionedIds = [...mentionedIds, emp.id];
		if (!assigneeId) assigneeId = emp.id;
		mentionMatch = null;
		mentionIdx = 0;
		void tick().then(() => {
			if (!descriptionInput) return;
			descriptionInput.focus();
			descriptionInput.setSelectionRange(applied.caret, applied.caret);
		});
	}

	function removeMention(id: string) {
		const next = mentionedIds.filter((item) => item !== id);
		mentionedIds = next;
		if (assigneeId === id) assigneeId = next[0] ?? '';
	}

	function handleDescriptionKeydown(event: KeyboardEvent) {
		if (!mentionMatch || mentionCandidates.length === 0) return;
		if (event.key === 'ArrowDown') {
			event.preventDefault();
			mentionIdx = (mentionIdx + 1) % mentionCandidates.length;
		} else if (event.key === 'ArrowUp') {
			event.preventDefault();
			mentionIdx = (mentionIdx - 1 + mentionCandidates.length) % mentionCandidates.length;
		} else if (event.key === 'Enter' || event.key === 'Tab') {
			event.preventDefault();
			pickMention(mentionCandidates[Math.min(mentionIdx, mentionCandidates.length - 1)]);
		} else if (event.key === 'Escape') {
			event.preventDefault();
			event.stopPropagation();
			mentionMatch = null;
		}
	}

	function appendDraftSteps(raw: string) {
		const lines = splitSteps(raw);
		const err = stepError(lines);
		if (err) {
			stepInputError = err;
			return;
		}
		draftSteps = [...draftSteps, ...lines];
		stepInput = '';
		stepInputError = undefined;
	}

	function handleStepKeydown(event: KeyboardEvent) {
		if (event.key !== 'Enter') return;
		event.preventDefault();
		if (stepInput.trim()) appendDraftSteps(stepInput);
	}

	function handleStepPaste(event: ClipboardEvent) {
		const pasted = event.clipboardData?.getData('text') ?? '';
		if (!pasted.includes('\n')) return;
		event.preventDefault();
		appendDraftSteps(stepInput ? `${stepInput}\n${pasted}` : pasted);
	}

	function removeDraftStep(index: number) {
		draftSteps = draftSteps.filter((_, i) => i !== index);
	}

	function askTessToScope() {
		if (!shellAi) return;
		const prompt = buildTessDraftPrompt(
			title,
			type === 'bug' ? whatHappened : description,
			draftSteps
		);
		open = false;
		shellAi.ask(prompt);
	}

	async function executeTessIntakeBackground(input: {
		jobId: string;
		payload: ReturnType<typeof buildCreatePayload>;
		stepsToAttach: string[];
		capturedTitle: string;
		capturedType: TaskType;
		mode: 'draft' | 'direct';
		assigneeName?: string;
	}) {
		try {
			const created = await workItemsApi.create(organizationId, input.payload);
			await persistDraftSteps(created.id, input.stepsToAttach);
			const isDraft = input.mode === 'draft';
			const summary = isDraft
				? `${input.capturedTitle} · Ready for Draft ideation`
				: `${input.capturedTitle}${input.assigneeName ? ` · Assigned to ${input.assigneeName}` : ''}`;
			notifications?.finishJob(input.jobId, {
				title: isDraft ? `Tess drafted ${created.ref}` : `Tess filed ${created.ref}`,
				summary,
				href: taskHref(created.ref),
				icon: isDraft ? 'pencil' : 'check',
				tone: isDraft ? 'accent' : 'ok'
			});
			oncreated?.({ ...created, type: input.capturedType });
			void shellData?.refresh();
		} catch (e) {
			const msg = toAppError(e).message;
			notifications?.failJob(input.jobId, `Couldn't file "${input.capturedTitle}"`, msg, '/work');
			toast.error(msg);
		}
	}

	/**
	 * Fire-and-forget async intake (TASK-139): closes the composer immediately, tracks progress
	 * in the top-bar Notifications bell + Floating Tess Companion pill, and creates the live
	 * work item at HQ (marking Draft ideation when exploratory).
	 */
	async function sendToTessAsync() {
		const blocked =
			busy || checking || Boolean(unknownOpId) || choicesUnavailable || type === 'bug';
		if (blocked) return;
		fieldErrors = {};
		formError = undefined;
		const nextErrors = validateDraft();
		if (Object.keys(nextErrors).length > 0) {
			fieldErrors = nextErrors;
			await focusField(Object.keys(nextErrors)[0]);
			return;
		}
		const routing = assessTessIntakeRouting(title, description, draftSteps, selectedAssigneeName);
		const capturedTitle = title.trim();
		const capturedType = type;
		const stepsToAttach = [...draftSteps, ...(stepInput.trim() ? splitSteps(stepInput) : [])];
		const op = operationId || newId();
		const basePayload = buildCreatePayload(op);
		const payload =
			routing.mode === 'draft'
				? { ...basePayload, description: withDraftIdeationMarker(basePayload.description) }
				: basePayload;
		const jobId = `tess-intake-${op}`;

		notifications?.startJob({
			id: jobId,
			title: `Refining: ${capturedTitle}`,
			step: routing.label,
			source: 'tess'
		});
		toast.message(`Sent "${capturedTitle}" to Tess — refining in background.`);

		if (createMore) {
			resetDraftFields(true);
			void tick().then(() => titleInput?.focus());
		} else {
			open = false;
		}

		await executeTessIntakeBackground({
			jobId,
			payload,
			stepsToAttach,
			capturedTitle,
			capturedType,
			mode: routing.mode,
			assigneeName: selectedAssigneeName
		});
	}

	async function persistDraftSteps(taskId: string, pending: string[]) {
		if (pending.length === 0) return;
		try {
			await workItemsApi.addSteps(organizationId, taskId, pending);
		} catch {
			toast.warning('Task created, but checklist steps could not be attached.');
		}
	}

	async function submit(event?: Event) {
		event?.preventDefault();
		if (busy || checking || unknownOpId || choicesUnavailable) return;
		fieldErrors = {};
		formError = undefined;

		const nextErrors = validateDraft();
		if (Object.keys(nextErrors).length > 0) {
			fieldErrors = nextErrors;
			await focusField(Object.keys(nextErrors)[0]);
			return;
		}

		const stepsToAttach = [...draftSteps, ...(stepInput.trim() ? splitSteps(stepInput) : [])];
		busy = true;
		const op = operationId || newId();
		operationId = op;
		try {
			const created = await workItemsApi.create(organizationId, buildCreatePayload(op));
			await persistDraftSteps(created.id, stepsToAttach);
			unknownOpId = undefined;
			toastCreated(created.ref);
			oncreated?.({ ...created, type });
			if (createMore) {
				resetDraftFields(true);
				void tick().then(() => titleInput?.focus());
			} else {
				open = false;
			}
		} catch (e) {
			await handleCreateError(e, op);
		} finally {
			busy = false;
		}
	}

	async function checkCreateStatus() {
		if (!unknownOpId || checking) return;
		checking = true;
		try {
			const res = await workItemsApi.createOperation(organizationId, unknownOpId);
			if (res.state === 'applied') {
				unknownOpId = undefined;
				open = false;
				toast.success(`${TYPE_WORD[type]} ${res.ref} saved.`);
				oncreated?.({ id: res.id, ref: res.ref, type });
			} else if (res.state === 'not_applied') {
				unknownOpId = undefined;
				formError = res.reason;
			} else {
				formError = 'Still waiting for confirmation from the office. Try Check status again.';
			}
		} catch (e) {
			formError = toAppError(e).message;
		} finally {
			checking = false;
		}
	}

	async function addToExisting(row: TaskRow) {
		if (addingId || busy) return;
		addingId = row.id;
		formError = undefined;
		try {
			const text =
				type === 'bug'
					? [title.trim(), whatHappened.trim(), steps.trim()].filter(Boolean).join('\n\n')
					: [title.trim(), description.trim()].filter(Boolean).join('\n\n');
			const res = await workItemsApi.addToExisting(organizationId, row.id, {
				text: text || title.trim(),
				operationId: newId()
			});
			open = false;
			toast.success(`Added as a shared comment on ${res.ref}.`);
			onadded?.(res.ref);
			await goto(resolve(taskHref(res.ref) as '/'));
		} catch (e) {
			formError = toAppError(e).message;
		} finally {
			addingId = null;
		}
	}

	function handleFormKeydown(event: KeyboardEvent) {
		if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
			event.preventDefault();
			if (event.shiftKey || type === 'bug' || !notifications) {
				if (!bugSubmitDisabledReason) void submit();
			} else {
				void sendToTessAsync();
			}
		}
	}
</script>

<Dialog bind:open onOpenChange={handleOpenChange}>
	<DialogContent class="work-composer-dialog max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-xl">
		<DialogHeader>
			<div class="flex items-start justify-between gap-3 pr-6">
				<DialogTitle text={dialogHeading} />
				{#if shellAi}
					<button
						type="button"
						onclick={askTessToScope}
						disabled={busy || checking}
						class="work-composer-tess-btn inline-flex shrink-0 items-center gap-1.5 rounded-md px-2.5 py-1 text-xs/5 font-medium"
						data-testid="composer-ask-tess"
					>
						<Sparkles class="size-3.5" aria-hidden="true" />
						<span>Ask Tess</span>
					</button>
				{/if}
			</div>
			<DialogDescription>
				{#if contextLabel}
					{contextLabel}
				{:else if type === 'bug'}
					Capture what happened with an accountable team. Severity can stay Not set.
				{:else if type === 'request'}
					Capture what you need and optionally route it to a team or employee to scope.
				{:else}
					Every task has one accountable team and an optional project, assigned team and assignee.
				{/if}
			</DialogDescription>
		</DialogHeader>

		{#if choicesState === 'unavailable'}
			<div
				class="rounded-md bg-surface-raised px-3 py-2.5 text-sm/6 text-fg-secondary ring-1 ring-line"
				data-testid="capture-choices-unavailable"
			>
				Creating work items isn't served on this server yet.
			</div>
		{:else if choicesState === 'unsupported'}
			<div
				class="rounded-md bg-surface-raised px-3 py-2.5 text-sm/6 text-fg-secondary ring-1 ring-line"
				data-testid="capture-choices-unsupported"
			>
				<p class="font-medium text-fg">Not available in this version</p>
				<p class="mt-0.5 text-xs/5 text-fg-muted">
					{choicesMessage || "This version of the office doesn't support creating work items."}
				</p>
			</div>
		{:else if choicesState === 'error'}
			<div
				class="flex flex-col gap-2 rounded-md bg-danger-emphasis/10 px-3 py-2.5 text-sm/6 text-danger-text ring-1 ring-danger-emphasis/20"
				role="alert"
				data-testid="capture-choices-error"
			>
				<p>Couldn't load teams and projects: {choicesMessage}</p>
				<div>
					<Button
						type="button"
						variant="outline"
						size="sm"
						class={mobileButton}
						onclick={() => loadChoices()}>Retry</Button
					>
				</div>
			</div>
		{/if}

		<!-- Cmd/Ctrl+Enter form shortcut handler --> <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
		<form class="flex flex-col gap-4" onsubmit={submit} onkeydown={handleFormKeydown} novalidate>
			{#if allowTypeSwitch && !fixesTaskId}
				<fieldset class="flex flex-col gap-1.5">
					<legend class="text-sm/6 font-medium text-fg">Type</legend>
					<div class="flex flex-wrap gap-2" role="radiogroup" aria-label="Work item type">
						{#each ['task', 'bug', 'request'] as const as itemType (itemType)}
							<label
								class="inline-flex cursor-pointer items-center gap-2 rounded-md px-3 py-1.5 text-sm/6 ring-1 ring-inset max-[639px]:min-h-11 {type ===
								itemType
									? 'bg-primary/10 font-semibold text-brand-text ring-brand/40'
									: 'bg-surface-raised text-fg-secondary ring-line hover:bg-surface-overlay'}"
							>
								<input
									type="radio"
									name="{uid}-type"
									value={itemType}
									checked={type === itemType}
									onchange={() => ((type = itemType), (fieldErrors = {}))}
									disabled={busy || !!unknownOpId}
									class="sr-only"
								/>
								{TYPE_WORD[itemType]}
							</label>
						{/each}
					</div>
				</fieldset>
			{/if}

			<!-- Title -->
			<div>
				<Label for="{uid}-title" class="block text-sm/6 font-medium text-fg">Title</Label>
				<Input
					id="{uid}-title"
					bind:ref={titleInput}
					bind:value={title}
					maxlength={120}
					disabled={busy || checking || !!unknownOpId}
					aria-invalid={fieldErrors.title ? 'true' : undefined}
					aria-describedby={fieldErrors.title ? `${uid}-title-err` : undefined}
					placeholder={type === 'bug'
						? 'Short summary of the bug'
						: type === 'request'
							? 'What do you need?'
							: 'What needs to be done?'}
					class="mt-1.5 {inputClass}"
				/>
				{#if fieldErrors.title}
					<p id="{uid}-title-err" class={errorClass} role="alert">{fieldErrors.title}</p>
				{/if}
			</div>

			<!-- Similar open bugs or requests (BUG-3, RQS-13) -->
			{#if similar.length > 0}
				<div
					class="flex flex-col gap-2 rounded-lg bg-surface-raised p-3 ring-1 ring-line"
					data-testid="similar-items"
				>
					<p class="text-xs/5 font-semibold text-fg">
						{type === 'bug' ? 'Similar open bugs' : 'Similar open requests'}
					</p>
					<ul role="list" class="flex flex-col gap-2">
						{#each similar as item (item.id)}
							<li class="flex flex-wrap items-center justify-between gap-2 text-xs/5">
								<div class="min-w-0 flex-1">
									<span class="font-mono text-fg-muted">{item.ref}</span>
									<span class="ml-1.5 text-fg">{item.title}</span>
									<span class="ml-1.5 text-fg-muted">
										({item.project ? item.project.name : 'No project'} · {item.team.name})
									</span>
								</div>
								<Button
									type="button"
									variant="outline"
									size="sm"
									class={mobileButton}
									disabled={addingId !== null || busy}
									onclick={() => addToExisting(item)}
								>
									{#if addingId === item.id}
										Adding…
									{:else if type === 'bug'}
										This is the same bug
									{:else}
										Add to {item.ref}
									{/if}
								</Button>
							</li>
						{/each}
					</ul>
				</div>
			{/if}

			{#if type === 'bug'}
				<!-- Bug-specific fields (work-items-ux §4) -->
				<div>
					<Label for="{uid}-what" class="block text-sm/6 font-medium text-fg">What happened</Label>
					<textarea
						id="{uid}-what"
						bind:this={whatHappenedInput}
						rows={3}
						bind:value={whatHappened}
						disabled={busy || checking || !!unknownOpId}
						aria-invalid={fieldErrors.whatHappened ? 'true' : undefined}
						aria-describedby={fieldErrors.whatHappened ? `${uid}-what-err` : undefined}
						placeholder="What went wrong and where did you see it?"
						class="mt-1.5 {textareaClass}"></textarea>
					{#if fieldErrors.whatHappened}
						<p id="{uid}-what-err" class={errorClass} role="alert">{fieldErrors.whatHappened}</p>
					{/if}
				</div>

				<!-- Severity with all four explanations + Not set (BUG-4) -->
				<fieldset class="flex flex-col gap-2">
					<legend class="text-sm/6 font-medium text-fg">Severity</legend>
					<div class="flex flex-col gap-1.5" role="radiogroup" aria-label="Severity">
						<label
							class="flex cursor-pointer items-start gap-2.5 rounded-md bg-surface-raised px-3 py-2 text-sm/6 ring-1 ring-line"
						>
							<input
								type="radio"
								name="{uid}-severity"
								value=""
								checked={severity === null}
								onchange={() => (severity = null)}
								disabled={busy || !!unknownOpId}
								class="mt-1 accent-primary"
							/>
							<span>
								<span class="font-medium text-fg">Not set</span>
								<span class="block text-xs/5 text-fg-muted">Leave severity to triage.</span>
							</span>
						</label>
						{#each SEVERITIES as sev (sev)}
							<label
								class="flex cursor-pointer items-start gap-2.5 rounded-md bg-surface-raised px-3 py-2 text-sm/6 ring-1 ring-line"
							>
								<input
									type="radio"
									name="{uid}-severity"
									value={sev}
									checked={severity === sev}
									onchange={() => (severity = sev)}
									disabled={busy || !!unknownOpId}
									class="mt-1 accent-primary"
								/>
								<span>
									<span class="font-medium text-fg">{SEVERITY_WORD[sev]}</span>
									<span class="block text-xs/5 text-fg-muted">{SEVERITY_HELP[sev]}</span>
								</span>
							</label>
						{/each}
					</div>
				</fieldset>

				<div class="grid gap-3 sm:grid-cols-2">
					<div>
						<Label for="{uid}-expected" class="block text-sm/6 font-medium text-fg">
							Expected result (optional)
						</Label>
						<textarea
							id="{uid}-expected"
							rows={2}
							bind:value={expected}
							disabled={busy || !!unknownOpId}
							class="mt-1.5 {textareaClass}"></textarea>
					</div>
					<div>
						<Label for="{uid}-steps" class="block text-sm/6 font-medium text-fg">
							Steps to reproduce (optional)
						</Label>
						<textarea
							id="{uid}-steps"
							rows={2}
							bind:value={steps}
							disabled={busy || !!unknownOpId}
							class="mt-1.5 {textareaClass}"></textarea>
					</div>
				</div>

				<div>
					<Label for="{uid}-env" class="block text-sm/6 font-medium text-fg">
						Environment (optional)
					</Label>
					<Input
						id="{uid}-env"
						bind:value={environment}
						disabled={busy || !!unknownOpId}
						placeholder="Office, browser or branch/commit if known"
						class="mt-1.5 {inputClass}"
					/>
				</div>
			{:else}
				<!-- Description / Request text with @-mention assignee picker -->
				<div class="relative">
					<Label for="{uid}-desc" class="block text-sm/6 font-medium text-fg">
						{type === 'request' ? 'Details and why it matters' : 'Description (optional)'}
					</Label>
					<textarea
						id="{uid}-desc"
						bind:this={descriptionInput}
						rows={4}
						value={description}
						oninput={handleDescriptionInput}
						onkeydown={handleDescriptionKeydown}
						disabled={busy || checking || !!unknownOpId}
						placeholder={type === 'request'
							? 'Describe the outcome you want… Type @ to tag an agent or person'
							: 'Context, acceptance notes, or links… Type @ to tag an agent or person'}
						class="mt-1.5 {textareaClass}"></textarea>

					{#if mentionMatch && mentionCandidates.length > 0}
						<div
							role="listbox"
							aria-label="Tag agent or person"
							class="work-mention-popover absolute right-0 left-0 z-30 mt-1 flex flex-col gap-0.5 p-1"
							data-testid="composer-mention-popover"
						>
							{#each mentionCandidates as emp, idx (emp.id)}
								<button
									type="button"
									role="option"
									aria-selected={idx === mentionIdx}
									onmousedown={(e) => {
										e.preventDefault();
										pickMention(emp);
									}}
									onmouseenter={() => (mentionIdx = idx)}
									class="work-mention-opt flex w-full items-center justify-between gap-2 rounded-md px-2.5 py-1.5 text-left text-xs/5 {idx ===
									mentionIdx
										? 'is-active'
										: ''}"
								>
									<span class="flex min-w-0 items-center gap-2">
										<span class="shrink-0 font-medium text-fg">{emp.name}</span>
										{#if emp.role}
											<span class="truncate text-fg-muted">{emp.role}</span>
										{/if}
									</span>
									<span class="shrink-0 text-[11px]/4 text-fg-muted">
										{emp.kind} · {emp.availability}
									</span>
								</button>
							{/each}
						</div>
					{/if}

					{#if mentionedEmployees.length > 0}
						<div class="mt-2 flex flex-wrap gap-1.5" aria-label="Tagged members">
							{#each mentionedEmployees as emp (emp.id)}
								<span
									class="inline-flex items-center gap-1 rounded-full bg-surface-raised px-2.5 py-0.5 text-xs/5 text-fg ring-1 ring-line"
								>
									<span>@{emp.name.split(' ')[0]}</span>
									{#if emp.role}
										<span class="text-fg-muted">{emp.role}</span>
									{/if}
									<button
										type="button"
										aria-label="Remove {emp.name}"
										onclick={() => removeMention(emp.id)}
										class="ml-0.5 rounded-full p-0.5 text-fg-muted hover:text-fg"
									>
										<X class="size-3" aria-hidden="true" />
									</button>
								</span>
							{/each}
						</div>
					{/if}
				</div>
			{/if}

			<!-- Inline Acceptance criteria / checklist steps -->
			<div class="flex flex-col gap-1.5">
				<Label for="{uid}-ac-step" class="block text-sm/6 font-medium text-fg">
					Acceptance criteria (optional)
				</Label>
				{#if draftSteps.length > 0}
					<ul role="list" class="flex flex-col gap-1" data-testid="composer-draft-steps">
						{#each draftSteps as stepText, idx (`${idx}-${stepText}`)}
							<li
								class="flex items-center justify-between gap-2 rounded-md bg-surface-raised px-2.5 py-1.5 text-xs/5 text-fg ring-1 ring-line"
							>
								<span class="min-w-0 flex-1 [overflow-wrap:anywhere]">{stepText}</span>
								<button
									type="button"
									aria-label="Remove step {idx + 1}"
									onclick={() => removeDraftStep(idx)}
									class="shrink-0 rounded p-0.5 text-fg-muted hover:text-fg"
								>
									<X class="size-3.5" aria-hidden="true" />
								</button>
							</li>
						{/each}
					</ul>
				{/if}
				<div class="flex items-center gap-2">
					<Input
						id="{uid}-ac-step"
						bind:value={stepInput}
						onkeydown={handleStepKeydown}
						onpaste={handleStepPaste}
						disabled={busy || checking || !!unknownOpId}
						placeholder="+ Add checklist step (Enter to add, paste lines for multiple)"
						class="flex-1 {inputClass}"
					/>
					<Button
						type="button"
						variant="outline"
						size="sm"
						disabled={busy || !stepInput.trim()}
						onclick={() => appendDraftSteps(stepInput)}
						class="shrink-0"
					>
						<Plus class="size-3.5" aria-hidden="true" />
						<span>Add</span>
					</Button>
				</div>
				{#if stepInputError}
					<p class={errorClass} role="alert">{stepInputError}</p>
				{/if}
			</div>

			<!-- Project & Accountable team -->
			<div class="grid gap-3 sm:grid-cols-2">
				<div>
					<Label for="{uid}-project" class="block text-sm/6 font-medium text-fg">
						Project (optional)
					</Label>
					<select
						id="{uid}-project"
						bind:value={chosenProjectId}
						disabled={busy || choicesUnavailable || !!unknownOpId}
						class="mt-1.5 {selectClass}"
					>
						<option value="">No project (team work)</option>
						{#each (choices?.projects ?? []).filter((p) => !p.archived) as p (p.id)}
							<option value={p.id}>{p.name} (P-{p.number})</option>
						{/each}
					</select>
				</div>

				<div>
					<Label for="{uid}-team" class="block text-sm/6 font-medium text-fg">
						Accountable team
					</Label>
					{#if selectedProject}
						<p
							class="mt-1.5 flex min-h-9 items-center rounded-md bg-surface-raised px-3 py-1.5 text-sm/6 text-fg-secondary outline-1 -outline-offset-1 outline-line"
							data-testid="derived-accountable-team"
						>
							{effectiveTeamName} <span class="ml-1.5 text-xs/5 text-fg-muted">(from project)</span>
						</p>
					{:else}
						<select
							id="{uid}-team"
							bind:this={teamSelect}
							bind:value={chosenTeamId}
							disabled={busy || choicesUnavailable || !!unknownOpId}
							aria-invalid={fieldErrors.team ? 'true' : undefined}
							aria-describedby={fieldErrors.team ? `${uid}-team-err` : undefined}
							class="mt-1.5 {selectClass}"
						>
							<option value="">Choose team…</option>
							{#each choices?.teams ?? [] as t (t.id)}
								<option value={t.id}>{t.path}</option>
							{/each}
						</select>
						{#if fieldErrors.team}
							<p id="{uid}-team-err" class={errorClass} role="alert">{fieldErrors.team}</p>
						{/if}
					{/if}
				</div>
			</div>

			<!-- Priority & Independent Assignment pair -->
			<div class="grid gap-3 sm:grid-cols-3">
				<div>
					<Label for="{uid}-priority" class="block text-sm/6 font-medium text-fg">Priority</Label>
					<select
						id="{uid}-priority"
						bind:value={priority}
						disabled={busy || !!unknownOpId}
						class="mt-1.5 {selectClass}"
					>
						{#each PRIORITIES as p (p)}
							<option value={p}>{PRIORITY_WORD[p]}</option>
						{/each}
					</select>
				</div>

				<div>
					<Label for="{uid}-assigned-team" class="block text-sm/6 font-medium text-fg">
						Assigned team
					</Label>
					<select
						id="{uid}-assigned-team"
						bind:value={assignedTeamId}
						disabled={busy || choicesUnavailable || !!unknownOpId}
						class="mt-1.5 {selectClass}"
					>
						<option value="">No assigned team</option>
						{#each choices?.teams ?? [] as t (t.id)}
							<option value={t.id}>{t.name}</option>
						{/each}
					</select>
				</div>

				<div>
					<Label for="{uid}-assignee" class="block text-sm/6 font-medium text-fg">Assignee</Label>
					<select
						id="{uid}-assignee"
						bind:value={assigneeId}
						disabled={busy || choicesUnavailable || !!unknownOpId}
						class="mt-1.5 {selectClass}"
					>
						<option value="">Unassigned</option>
						{#each choices?.employees ?? [] as emp (emp.id)}
							<option value={emp.id}>
								{emp.name} ({emp.kind}{emp.availability ? ` · ${emp.availability}` : ''})
							</option>
						{/each}
					</select>
				</div>
			</div>

			{#if bugSubmitDisabledReason}
				<p class={helpClass} data-testid="bug-submit-reason">{bugSubmitDisabledReason}</p>
			{/if}

			{#if tessRouting && !unknownOpId}
				<p
					class="flex items-center gap-1.5 text-xs/5 text-fg-muted"
					data-testid="composer-tess-routing"
				>
					<Sparkles class="size-3.5 shrink-0 text-brand-text" aria-hidden="true" />
					<span>{tessRouting.label}</span>
				</p>
			{/if}

			{#if formError}
				<div
					class="rounded-md bg-danger-emphasis/10 px-3 py-2.5 text-sm/6 text-danger-text ring-1 ring-danger-emphasis/20"
					role="alert"
				>
					{formError}
				</div>
			{/if}

			<DialogFooter class="flex-wrap items-center justify-between gap-3 sm:justify-between">
				<label class="inline-flex cursor-pointer items-center gap-2 text-xs/5 text-fg-secondary">
					<button
						type="button"
						role="switch"
						aria-checked={createMore}
						aria-label="Create more"
						disabled={busy || checking || !!unknownOpId}
						onclick={() => (createMore = !createMore)}
						class="work-composer-switch"
						data-testid="composer-create-more"
					></button>
					<span>Create more</span>
				</label>
				<div class="flex flex-wrap items-center gap-2">
					<Button
						type="button"
						variant="outline"
						class={mobileButton}
						disabled={busy || checking}
						onclick={() => handleOpenChange(false)}>Cancel</Button
					>
					{#if unknownOpId}
						<Button
							type="button"
							variant="outline"
							class={mobileButton}
							disabled={checking}
							onclick={discardAttempt}>Discard attempt</Button
						>
						<Button
							type="button"
							class={mobileButton}
							disabled={checking}
							onclick={checkCreateStatus}>{checking ? 'Checking…' : 'Check status'}</Button
						>
					{:else}
						{#if type !== 'bug' && notifications}
							<Button
								type="button"
								variant="outline"
								class={mobileButton}
								disabled={busy || choicesUnavailable}
								onclick={() => void sendToTessAsync()}
								data-testid="composer-send-to-tess"
							>
								<Sparkles class="size-3.5" aria-hidden="true" />
								<span>Send to Tess</span>
							</Button>
						{/if}
						<Button
							type="submit"
							class={mobileButton}
							disabled={busy || choicesUnavailable || !!bugSubmitDisabledReason}
							aria-busy={busy}
						>
							{submitLabel}
						</Button>
					{/if}
				</div>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

<ConfirmDialog
	bind:open={discardOpen}
	title={type === 'bug'
		? 'Discard this bug report?'
		: type === 'request'
			? 'Discard this unsaved request?'
			: 'Discard this unsaved task?'}
	description="Your unsaved text will be lost."
	confirmLabel="Discard"
	busyLabel="Discarding…"
	destructive
	onconfirm={confirmDiscard}
/>

<style>
	:global(.work-composer-dialog) {
		border-radius: var(--exec-cmdk-dialog-radius, 12px);
		background: var(--exec-cmdk-menu-bg, var(--surface));
		-webkit-backdrop-filter: var(--exec-cmdk-menu-blur, none);
		backdrop-filter: var(--exec-cmdk-menu-blur, none);
	}
	:global(.work-composer-dialog :is(input, textarea, select)) {
		background-color: var(--exec-well, var(--surface-raised));
		outline-color: var(--exec-border-hi, var(--line-strong));
	}
	.work-composer-tess-btn {
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		background: var(--exec-well, var(--surface-raised));
		color: var(--exec-body, var(--fg-secondary));
		cursor: pointer;
	}
	.work-composer-tess-btn:hover {
		background: var(--exec-surface-hi, var(--surface-overlay));
		color: var(--exec-ink, var(--fg));
	}
	.work-mention-popover {
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--popover, var(--surface-overlay));
		-webkit-backdrop-filter: blur(18px);
		backdrop-filter: blur(18px);
		box-shadow: var(--exec-cmdk-menu-shadow, 0 12px 24px -8px var(--surface-sunken));
	}
	.work-mention-opt {
		cursor: pointer;
	}
	.work-mention-opt:hover,
	.work-mention-opt.is-active {
		background: var(--exec-cmdk-opt-active-bg, var(--surface-raised));
	}
	.work-composer-switch {
		position: relative;
		width: 2rem;
		height: 1.125rem;
		border-radius: 9999px;
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		background: var(--exec-well, var(--surface-raised));
		cursor: pointer;
		transition: background-color 140ms ease-out;
	}
	.work-composer-switch::after {
		content: '';
		position: absolute;
		top: 2px;
		left: 2px;
		width: 0.75rem;
		height: 0.75rem;
		border-radius: 9999px;
		background: var(--exec-muted, var(--fg-muted));
		transition: transform 140ms ease-out;
	}
	.work-composer-switch[aria-checked='true'] {
		background: var(--tone-accent, var(--primary));
		border-color: var(--tone-accent, var(--primary));
	}
	.work-composer-switch[aria-checked='true']::after {
		transform: translateX(0.875rem);
		background: var(--on-emphasis);
	}
	.work-composer-switch:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
</style>
