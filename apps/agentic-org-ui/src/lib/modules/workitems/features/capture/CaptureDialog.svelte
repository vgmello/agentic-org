<!--
  @component
  Capture dialog for New task, Report a bug, New request, Create fix task and top-bar New › Work item
  (work-items-ux §4, §9; feature-requests-ux §1; BUG-2..9, TSK-5, RQS-1..3, RQS-13, RQS-17).
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import { newId } from '$lib/modules/organization/commands';
	import { useNotifications } from '$lib/platform/shell/notifications.svelte';
	import { useShellAi } from '$lib/platform/shell/shell-ai';
	import { useShellData } from '$lib/platform/shell/shell-data.svelte';
	import { toAppError } from '$lib/proto';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import { errorClass, helpClass, inputClass } from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import { OUTCOME_UNKNOWN } from '../../budget';
	import { withDraftIdeationMarker } from '../../ideation';
	import { TYPE_WORD, splitSteps, taskHref } from '../../present';
	import type { Choices, Priority, Severity, TaskRow, TaskType } from '../../types';
	import { assessTessIntakeRouting, buildTessDraftPrompt } from './composer-helpers';
	import {
		buildCapturePayload,
		buildExistingCommentText,
		classifyChoicesError,
		formatCreatedToast,
		getBugSubmitDisabledReason,
		persistDraftSteps,
		runTessIntakeBackground,
		validateCaptureDraft,
		type ChoicesState
	} from './capture-helpers';
	import CaptureBugFields from './CaptureBugFields.svelte';
	import CaptureChecklistField from './CaptureChecklistField.svelte';
	import CaptureChoicesBanner from './CaptureChoicesBanner.svelte';
	import CaptureDescriptionField from './CaptureDescriptionField.svelte';
	import CaptureFooter from './CaptureFooter.svelte';
	import CaptureScopeFields from './CaptureScopeFields.svelte';
	import CaptureSimilarList from './CaptureSimilarList.svelte';

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
	let mentionedIds = $state<string[]>([]);

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
	let whatHappenedInput = $state<HTMLTextAreaElement | null>(null);
	let teamSelect = $state<HTMLSelectElement | null>(null);
	let descField = $state<ReturnType<typeof CaptureDescriptionField>>();
	let checklistField = $state<ReturnType<typeof CaptureChecklistField>>();

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

	const bugSubmitDisabledReason = $derived(getBugSubmitDisabledReason(type, title, whatHappened));
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
		mentionedIds = [];
		descField?.resetMentions();
		checklistField?.resetError();
		similar = [];
		fieldErrors = {};
		formError = undefined;
		operationId = newId();
	}

	$effect(() => {
		if (!open || unknownOpId) return;
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
		if (next || busy || checking) {
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

	function currentPayload(op: string) {
		return buildCapturePayload({
			operationId: op,
			type,
			title,
			description,
			whatHappened,
			expected,
			steps,
			environment,
			chosenProjectId,
			effectiveTeamId,
			assignedTeamId,
			assigneeId,
			priority,
			severity,
			fixesTaskId,
			relatesToTaskId
		});
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

	async function sendToTessAsync() {
		if (busy || checking || unknownOpId || choicesUnavailable || type === 'bug') return;
		fieldErrors = {};
		formError = undefined;
		const nextErrors = validateCaptureDraft({ type, title, whatHappened, effectiveTeamId });
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
		const basePayload = currentPayload(op);
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

		await runTessIntakeBackground({
			organizationId,
			jobId,
			payload,
			stepsToAttach,
			capturedTitle,
			capturedType,
			mode: routing.mode,
			assigneeName: selectedAssigneeName,
			finishJob: (id, meta) => notifications?.finishJob(id, meta),
			failJob: (id, label, err, href) => notifications?.failJob(id, label, err, href),
			oncreated,
			onrefresh: () => void shellData?.refresh()
		});
	}

	async function submit(event?: Event) {
		event?.preventDefault();
		if (busy || checking || unknownOpId || choicesUnavailable) return;
		fieldErrors = {};
		formError = undefined;

		const nextErrors = validateCaptureDraft({ type, title, whatHappened, effectiveTeamId });
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
			const created = await workItemsApi.create(organizationId, currentPayload(op));
			await persistDraftSteps(organizationId, created.id, stepsToAttach);
			unknownOpId = undefined;
			toast.success(formatCreatedToast(type, created.ref, choices, assigneeId));
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
			const res = await workItemsApi.addToExisting(organizationId, row.id, {
				text: buildExistingCommentText({ type, title, description, whatHappened, steps }),
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
		if (!(event.metaKey || event.ctrlKey) || event.key !== 'Enter') return;
		event.preventDefault();
		if (event.shiftKey || type === 'bug' || !notifications) {
			if (!bugSubmitDisabledReason) void submit();
		} else {
			void sendToTessAsync();
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

		<CaptureChoicesBanner {choicesState} {choicesMessage} onretry={() => void loadChoices()} />
		<!-- Cmd/Ctrl+Enter form shortcut handler -->
		<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
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

			<CaptureSimilarList {type} {similar} {addingId} {busy} onadd={addToExisting} />

			{#if type === 'bug'}
				<CaptureBugFields
					{uid}
					disabled={busy || !!unknownOpId}
					{checking}
					whatHappenedError={fieldErrors.whatHappened}
					bind:whatHappened
					bind:expected
					bind:steps
					bind:environment
					bind:severity
					bind:whatHappenedInput
				/>
			{:else}
				<CaptureDescriptionField
					bind:this={descField}
					{uid}
					{type}
					employees={choices?.employees ?? []}
					disabled={busy || checking || !!unknownOpId}
					bind:description
					bind:mentionedIds
					bind:assigneeId
				/>
			{/if}

			<CaptureChecklistField
				bind:this={checklistField}
				{uid}
				disabled={busy || checking || !!unknownOpId}
				{busy}
				bind:draftSteps
				bind:stepInput
			/>

			<CaptureScopeFields
				{uid}
				{choices}
				{choicesUnavailable}
				disabled={busy || !!unknownOpId}
				{selectedProject}
				{effectiveTeamName}
				teamError={fieldErrors.team}
				bind:chosenProjectId
				bind:chosenTeamId
				bind:priority
				bind:assignedTeamId
				bind:assigneeId
				bind:teamSelect
			/>

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

			<CaptureFooter
				{type}
				{busy}
				{checking}
				{unknownOpId}
				{choicesUnavailable}
				{bugSubmitDisabledReason}
				{submitLabel}
				showSendToTess={Boolean(notifications)}
				bind:createMore
				oncancel={() => handleOpenChange(false)}
				ondiscard={discardAttempt}
				oncheckstatus={checkCreateStatus}
				onsendtotess={() => void sendToTessAsync()}
			/>
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
</style>
