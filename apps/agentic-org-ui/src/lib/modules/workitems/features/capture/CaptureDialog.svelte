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
	import { newId } from '$lib/modules/organization/commands';
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
	import {
		PRIORITIES,
		PRIORITY_WORD,
		SEVERITIES,
		SEVERITY_HELP,
		SEVERITY_WORD,
		TYPE_WORD,
		taskHref,
		titleError
	} from '../../present';
	import type { Choices, Priority, Severity, TaskRow, TaskType } from '../../types';

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
	let whatHappenedInput = $state<HTMLTextAreaElement | null>(null);
	let teamSelect = $state<HTMLSelectElement | null>(null);

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
			steps.trim().length > 0
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
			}
		} catch (e) {
			const classified = classifyChoicesError(e);
			choices = null;
			choicesState = classified.state;
			choicesMessage = classified.message;
		}
	}

	$effect(() => {
		if (!open) return;
		if (unknownOpId) return;
		type = initialType;
		title = '';
		description = '';
		whatHappened = '';
		expected = '';
		steps = '';
		environment = '';
		severity = null;
		priority = 'normal';
		chosenProjectId = projectId ?? '';
		chosenTeamId = '';
		assignedTeamId = '';
		assigneeId = '';
		similar = [];
		fieldErrors = {};
		formError = undefined;
		operationId = newId();
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

		busy = true;
		const op = operationId || newId();
		operationId = op;
		try {
			const created = await workItemsApi.create(organizationId, buildCreatePayload(op));
			unknownOpId = undefined;
			open = false;
			toastCreated(created.ref);
			oncreated?.({ ...created, type });
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
			if (!bugSubmitDisabledReason) void submit();
		}
	}
</script>

<Dialog bind:open onOpenChange={handleOpenChange}>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-xl">
		<DialogHeader>
			<DialogTitle text={dialogHeading} />
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
				<!-- Description / Request text -->
				<div>
					<Label for="{uid}-desc" class="block text-sm/6 font-medium text-fg">
						{type === 'request' ? 'Details and why it matters' : 'Description (optional)'}
					</Label>
					<textarea
						id="{uid}-desc"
						rows={4}
						bind:value={description}
						disabled={busy || checking || !!unknownOpId}
						placeholder={type === 'request'
							? 'Describe the outcome you want and any context…'
							: 'Context, acceptance notes, or links…'}
						class="mt-1.5 {textareaClass}"></textarea>
				</div>
			{/if}

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

			{#if formError}
				<div
					class="rounded-md bg-danger-emphasis/10 px-3 py-2.5 text-sm/6 text-danger-text ring-1 ring-danger-emphasis/20"
					role="alert"
				>
					{formError}
				</div>
			{/if}

			<DialogFooter>
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
					<Button type="button" class={mobileButton} disabled={checking} onclick={checkCreateStatus}
						>{checking ? 'Checking…' : 'Check status'}</Button
					>
				{:else}
					<Button
						type="submit"
						class={mobileButton}
						disabled={busy || choicesUnavailable || !!bugSubmitDisabledReason}
						aria-busy={busy}
					>
						{submitLabel}
					</Button>
				{/if}
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
