<!--
  @component
  Bug-specific sections on /work/[ref] (work-items-ux §5, §7; TRI-1..6, FIX-1..9, BUG-14):
  1. Bug report details (what happened, expected, steps, environment, original reported severity/priority & reporter)
  2. Triage region (confirm severity, priority & atomic assignment pair with expected revision conflict handling)
  3. Fix tasks (derived fixed_by + Create fix task), Mark as fixed dialog, Verify (Fixed / Still happens with note)
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
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
	import {
		errorClass,
		inputClass,
		mobileButton,
		selectClass,
		textareaClass
	} from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import {
		PRIORITIES,
		PRIORITY_WORD,
		SEVERITIES,
		SEVERITY_WORD,
		STATUS_WORD,
		assignmentText,
		linkText,
		severityWord,
		taskHref,
		when
	} from '../../present';
	import type {
		AssignmentConflict,
		BugDetails,
		Choices,
		FixHow,
		Priority,
		Severity,
		TaskDetail
	} from '../../types';
	import WorkIcon from '../../components/WorkIcon.svelte';

	let {
		organizationId,
		task,
		bug,
		choices,
		stale = false,
		oncreatefix,
		onchanged
	}: {
		organizationId: string;
		task: TaskDetail;
		bug: BugDetails;
		choices: Choices | null;
		stale?: boolean;
		oncreatefix: () => void;
		onchanged: () => void;
	} = $props();

	const uid = $props.id();

	// Triage form state (writable $derived resets cleanly when task updates)
	let triageSeverity = $derived<Severity | ''>(task.severity ?? '');
	let triagePriority = $derived<Priority>(task.priority);
	let triageTeamId = $derived(task.assignedTeam?.id ?? '');
	let triageAssigneeId = $derived(task.assignee?.id ?? '');
	let triageBusy = $state(false);
	let triageError = $state<string | undefined>();
	let triageConflict = $state<AssignmentConflict | null>(null);

	const missingAssignedTeam = $derived(
		choices !== null &&
			!!task.assignedTeam &&
			!choices.teams.some((t) => t.id === task.assignedTeam!.id)
	);
	const missingAssignee = $derived(
		choices !== null &&
			!!task.assignee &&
			!choices.employees.some((e) => e.id === task.assignee!.id)
	);

	async function saveTriage(expectedRevision: number) {
		if (triageBusy || stale) return;
		triageBusy = true;
		triageError = undefined;
		try {
			await workItemsApi.triage(organizationId, task.id, {
				severity: triageSeverity || null,
				priority: triagePriority,
				assignedTeamId: triageTeamId || null,
				assigneeId: triageAssigneeId || null,
				expectedRevision
			});
			triageConflict = null;
			toast.success(`Triaged ${task.ref}.`);
			onchanged();
		} catch (e) {
			const raw = e as { code?: string; current?: AssignmentConflict };
			if (raw?.code === 'conflict' && raw.current) {
				triageConflict = raw.current;
			} else {
				triageError = toAppError(e).message;
			}
		} finally {
			triageBusy = false;
		}
	}

	// Mark as fixed dialog state (FIX-3)
	let markFixedOpen = $state(false);
	let fixKind = $state<'task' | 'commit' | 'note'>('note');
	let fixTaskRef = $state('');
	let fixCommit = $state('');
	let fixNote = $state('');
	let fixBusy = $state(false);
	let fixError = $state<string | undefined>();

	async function submitMarkFixed(event: SubmitEvent) {
		event.preventDefault();
		if (fixBusy || stale) return;
		fixError = undefined;
		let how: FixHow;
		if (fixKind === 'task') {
			if (!fixTaskRef.trim()) {
				fixError = 'Enter the fix task reference (for example T-A40).';
				return;
			}
			how = { kind: 'task', taskRef: fixTaskRef.trim().toUpperCase() };
		} else if (fixKind === 'commit') {
			if (!fixCommit.trim()) {
				fixError = 'Enter the commit SHA.';
				return;
			}
			how = { kind: 'commit', commit: fixCommit.trim() };
		} else {
			if (!fixNote.trim()) {
				fixError = 'Describe what fixed the bug.';
				return;
			}
			how = { kind: 'note' };
		}
		fixBusy = true;
		try {
			await workItemsApi.markFixed(organizationId, task.id, {
				how,
				note: fixNote.trim(),
				expectedRevision: task.revision
			});
			markFixedOpen = false;
			fixNote = '';
			fixTaskRef = '';
			fixCommit = '';
			toast.success(`Recorded fix evidence on ${task.ref}.`);
			onchanged();
		} catch (e) {
			fixError = toAppError(e).message;
		} finally {
			fixBusy = false;
		}
	}

	// Verify dialog state (FIX-4, FIX-5)
	let verifyOpen = $state(false);
	let verifyResult = $state<'fixed' | 'still_happens'>('fixed');
	let verifyNote = $state('');
	let verifyBusy = $state(false);
	let verifyError = $state<string | undefined>();

	async function submitVerify(event: SubmitEvent) {
		event.preventDefault();
		if (verifyBusy || stale) return;
		if (verifyResult === 'still_happens' && !verifyNote.trim()) {
			verifyError = 'Explain what still happens so the team can follow up.';
			return;
		}
		verifyBusy = true;
		verifyError = undefined;
		try {
			await workItemsApi.verify(organizationId, task.id, {
				result: verifyResult,
				note: verifyNote.trim(),
				expectedRevision: task.revision
			});
			verifyOpen = false;
			verifyNote = '';
			toast.success(
				verifyResult === 'fixed'
					? `Verified ${task.ref} as fixed.`
					: `Reopened ${task.ref}: still happens.`
			);
			onchanged();
		} catch (e) {
			verifyError = toAppError(e).message;
		} finally {
			verifyBusy = false;
		}
	}
</script>

<!-- 1. Bug report details & original provenance (TRI-4) -->
<section
	aria-labelledby="{uid}-report-heading"
	class="flex flex-col gap-3 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
	data-testid="bug-report-section"
>
	<div class="flex flex-wrap items-baseline justify-between gap-2">
		<h3 id="{uid}-report-heading" class="text-sm/6 font-semibold text-fg">Bug report</h3>
		<p class="text-xs/5 text-fg-muted" data-testid="reported-provenance">
			Originally reported by {bug.report.reported.by.name} ({bug.report.reported.by.kind}) · {when(
				bug.report.reported.at
			)} · severity {severityWord(bug.report.reported.severity)}, priority {PRIORITY_WORD[
				bug.report.reported.priority
			]}
		</p>
	</div>

	<dl class="flex flex-col gap-3 text-sm/6">
		<div>
			<dt class="text-xs/5 font-medium text-fg-muted">What happened</dt>
			<dd class="mt-0.5 whitespace-pre-wrap text-fg-body [overflow-wrap:anywhere]">
				{bug.report.whatHappened}
			</dd>
		</div>
		{#if bug.report.expected}
			<div>
				<dt class="text-xs/5 font-medium text-fg-muted">Expected result</dt>
				<dd class="mt-0.5 whitespace-pre-wrap text-fg-body [overflow-wrap:anywhere]">
					{bug.report.expected}
				</dd>
			</div>
		{/if}
		{#if bug.report.steps}
			<div>
				<dt class="text-xs/5 font-medium text-fg-muted">Steps to reproduce</dt>
				<dd class="mt-0.5 whitespace-pre-wrap text-fg-body [overflow-wrap:anywhere]">
					{bug.report.steps}
				</dd>
			</div>
		{/if}
		{#if bug.report.environment}
			<div>
				<dt class="text-xs/5 font-medium text-fg-muted">Environment</dt>
				<dd class="mt-0.5 font-mono text-xs/5 text-fg-secondary [overflow-wrap:anywhere]">
					{bug.report.environment}
				</dd>
			</div>
		{/if}
	</dl>

	{#if bug.duplicateOf || bug.closedReason}
		<div class="flex flex-col gap-1 border-t border-line-subtle pt-3 text-xs/5">
			{#if bug.duplicateOf}
				<p class="text-fg-secondary" data-testid="duplicate-of">
					Duplicate of
					<a
						href={resolve(taskHref(bug.duplicateOf.ref) as '/')}
						class="ml-1 font-medium text-link hover:underline"
					>
						{linkText(bug.duplicateOf)}
					</a>
				</p>
			{/if}
			{#if bug.closedReason}
				<p class="text-fg-secondary">Closure note: {bug.closedReason}</p>
			{/if}
		</div>
	{/if}
</section>

<!-- 2. Triage region (TRI-1..6) -->
{#if task.can.triage && !stale}
	<section
		aria-label="Triage"
		class="flex flex-col gap-3 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
		data-testid="triage-region"
	>
		<div class="flex flex-wrap items-baseline justify-between gap-2">
			<h3 class="text-sm/6 font-semibold text-fg">
				Triage
				{#if bug.severityConfirmed}
					<span class="ml-1.5 text-xs/5 font-normal text-fg-muted">(severity confirmed)</span>
				{/if}
			</h3>
			{#if bug.suggestedTriager}
				<span class="text-xs/5 text-fg-muted">
					Suggested triager: {bug.suggestedTriager.name} ({bug.suggestedTriager.kind})
				</span>
			{/if}
		</div>

		<p class="text-xs/5 text-fg-muted">
			Accountable team: <span class="font-medium text-fg-secondary">{task.team.name}</span>
			(read-only here). Assigned team and assignee save atomically at revision {task.revision}.
		</p>

		<div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
			<div>
				<Label for="{uid}-triage-sev" class="block text-xs/5 font-medium text-fg">Severity</Label>
				<select
					id="{uid}-triage-sev"
					bind:value={triageSeverity}
					disabled={triageBusy}
					class="mt-1 {selectClass}"
				>
					<option value="">Not set</option>
					{#each SEVERITIES as s (s)}
						<option value={s}>{SEVERITY_WORD[s]}</option>
					{/each}
				</select>
			</div>

			<div>
				<Label for="{uid}-triage-pri" class="block text-xs/5 font-medium text-fg">Priority</Label>
				<select
					id="{uid}-triage-pri"
					bind:value={triagePriority}
					disabled={triageBusy}
					class="mt-1 {selectClass}"
				>
					{#each PRIORITIES as p (p)}
						<option value={p}>{PRIORITY_WORD[p]}</option>
					{/each}
				</select>
			</div>

			<div>
				<Label for="{uid}-triage-team" class="block text-xs/5 font-medium text-fg">
					Assigned team
				</Label>
				<select
					id="{uid}-triage-team"
					bind:value={triageTeamId}
					disabled={triageBusy}
					class="mt-1 {selectClass}"
				>
					<option value="">No assigned team</option>
					{#if choices === null && task.assignedTeam}
						<option value={task.assignedTeam.id}>{task.assignedTeam.name}</option>
					{:else if missingAssignedTeam && task.assignedTeam}
						<option value={task.assignedTeam.id} disabled>
							{task.assignedTeam.name} (archived)
						</option>
					{/if}
					{#each choices?.teams ?? [] as t (t.id)}
						<option value={t.id}>{t.name}</option>
					{/each}
				</select>
			</div>

			<div>
				<Label for="{uid}-triage-assignee" class="block text-xs/5 font-medium text-fg">
					Assignee
				</Label>
				<select
					id="{uid}-triage-assignee"
					bind:value={triageAssigneeId}
					disabled={triageBusy}
					class="mt-1 {selectClass}"
				>
					<option value="">Unassigned individual</option>
					{#if choices === null && task.assignee}
						<option value={task.assignee.id}>
							{task.assignee.name} ({task.assignee.kind}{task.assignee.former ? ' · former' : ''})
						</option>
					{:else if missingAssignee && task.assignee}
						<option value={task.assignee.id} disabled>
							{task.assignee.name} ({task.assignee.kind}{task.assignee.former ? ' · former' : ''})
						</option>
					{/if}
					{#each choices?.employees ?? [] as emp (emp.id)}
						<option value={emp.id}>{emp.name} ({emp.kind})</option>
					{/each}
				</select>
			</div>
		</div>

		{#if triageConflict}
			<div
				class="flex flex-col gap-2 rounded-lg bg-danger-emphasis/10 p-3 text-xs/5 text-danger-text ring-1 ring-danger-emphasis/20"
				role="alert"
				data-testid="triage-conflict"
			>
				<p class="font-semibold">
					{task.ref} changed while you were triaging (by {triageConflict.by.name} · {when(
						triageConflict.at
					)}).
				</p>
				<p class="text-fg-secondary">
					Current assignment: {assignmentText(triageConflict)} (revision {triageConflict.revision}).
					Your desired choice is kept above.
				</p>
				<div class="flex flex-wrap gap-2">
					<Button
						type="button"
						size="sm"
						class={mobileButton}
						disabled={triageBusy}
						onclick={() => saveTriage(triageConflict!.revision)}
					>
						Retry at revision {triageConflict.revision}
					</Button>
					<Button
						type="button"
						variant="outline"
						size="sm"
						class={mobileButton}
						onclick={() => ((triageConflict = null), onchanged())}
					>
						Discard and reload
					</Button>
				</div>
			</div>
		{:else}
			{#if triageError}
				<p class={errorClass} role="alert">{triageError}</p>
			{/if}
			<div class="flex justify-end">
				<Button
					type="button"
					size="sm"
					class={mobileButton}
					disabled={triageBusy}
					onclick={() => saveTriage(task.revision)}
				>
					{triageBusy ? 'Saving triage…' : 'Save triage'}
				</Button>
			</div>
		{/if}
	</section>
{/if}

<!-- 3. Fix tasks, Mark as fixed & Verify (FIX-1..6, BUG-14) -->
<section
	aria-labelledby="{uid}-fix-heading"
	class="flex flex-col gap-3 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
	data-testid="bug-fix-section"
>
	<div class="flex flex-wrap items-center justify-between gap-2">
		<h3 id="{uid}-fix-heading" class="text-sm/6 font-semibold text-fg">Fix and verification</h3>
		{#if !stale}
			<div class="flex flex-wrap gap-2">
				<Button type="button" variant="outline" size="sm" class={mobileButton} onclick={oncreatefix}
					>Create fix task</Button
				>
				<Button
					type="button"
					variant="outline"
					size="sm"
					class={mobileButton}
					onclick={() => ((fixError = undefined), (markFixedOpen = true))}>Mark as fixed…</Button
				>
				{#if bug.fixed && task.can.verify}
					<Button
						type="button"
						size="sm"
						class={mobileButton}
						onclick={() => ((verifyError = undefined), (verifyOpen = true))}
					>
						Verify fix…
					</Button>
				{/if}
			</div>
		{/if}
	</div>

	{#if bug.fixed}
		<div
			class="flex flex-col gap-1 rounded-lg bg-surface p-3 text-sm/6 outline-1 -outline-offset-1 outline-line-subtle"
			data-testid="bug-fixed-evidence"
		>
			<p class="font-medium text-fg">
				Marked as fixed by {bug.fixed.by.name} ({bug.fixed.by.kind}) · {when(bug.fixed.at)}
			</p>
			<p class="text-xs/5 text-fg-secondary">
				{#if bug.fixed.how.kind === 'task'}
					Fix task:
					<a
						href={resolve(taskHref(bug.fixed.how.task.ref) as '/')}
						class="text-link hover:underline"
					>
						{linkText(bug.fixed.how.task)}
					</a>
				{:else if bug.fixed.how.kind === 'commit'}
					Commit: <span class="font-mono">{bug.fixed.how.commit}</span>
				{:else}
					Note recorded
				{/if}
				{#if bug.fixed.note}
					· {bug.fixed.note}
				{/if}
			</p>
			{#if bug.suggestedVerifier}
				<p class="text-xs/5 text-fg-muted">
					Suggested verifier: {bug.suggestedVerifier.name} ({bug.suggestedVerifier.kind})
				</p>
			{/if}
		</div>
	{/if}

	{#if bug.verifications.length > 0}
		<div class="flex flex-col gap-1.5" data-testid="bug-verifications">
			<p class="text-xs/5 font-medium text-fg-muted">Verification history</p>
			<ul role="list" class="flex flex-col gap-1.5 text-xs/5">
				{#each bug.verifications as v, idx (`${v.at}-${idx}`)}
					<li class="text-fg-secondary">
						<span class="font-medium text-fg">
							{v.result === 'fixed' ? 'Verified fixed' : 'Still happens'}
						</span>
						by {v.by.name} ({v.by.kind}) · {when(v.at)}
						{#if v.note}
							— {v.note}
						{/if}
					</li>
				{/each}
			</ul>
		</div>
	{/if}

	<!-- Derived fixed_by tasks -->
	<div class="flex flex-col gap-1.5">
		<p class="text-xs/5 font-medium text-fg-muted">
			Linked fix tasks ({bug.fixTasks.length})
		</p>
		{#if bug.fixTasks.length === 0}
			<p class="text-xs/5 text-fg-muted">
				No linked fix tasks yet. Linked task progress stays evidence and never closes the bug
				automatically.
			</p>
		{:else}
			<ul role="list" class="flex flex-col gap-1.5" data-testid="linked-fix-tasks">
				{#each bug.fixTasks as ft (ft.id)}
					<li class="flex flex-wrap items-center justify-between gap-2 text-sm/6">
						<a
							href={resolve(taskHref(ft.ref) as '/')}
							class="inline-flex items-center gap-1.5 text-link hover:underline"
						>
							<WorkIcon name={ft.type} class="size-3.5 text-fg-muted" />
							<span>{linkText(ft)}</span>
						</a>
						<span class="text-xs/5 text-fg-muted">{STATUS_WORD[ft.status]}</span>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</section>

<!-- Mark as fixed modal -->
<Dialog bind:open={markFixedOpen} onOpenChange={(n) => !n && fixBusy && (markFixedOpen = true)}>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
		<DialogHeader>
			<DialogTitle text="Mark {task.ref} as fixed" />
			<DialogDescription>
				Records fix evidence with your actor and time in history. Move to Done separately if ready.
			</DialogDescription>
		</DialogHeader>
		<form class="flex flex-col gap-4" onsubmit={submitMarkFixed} novalidate>
			<div>
				<Label for="{uid}-fix-how" class="block text-sm/6 font-medium text-fg">What fixed it</Label>
				<select
					id="{uid}-fix-how"
					bind:value={fixKind}
					disabled={fixBusy}
					class="mt-1.5 {selectClass}"
				>
					<option value="note">Note</option>
					<option value="task">Linked fix task</option>
					<option value="commit">Commit</option>
				</select>
			</div>

			{#if fixKind === 'task'}
				<div>
					<Label for="{uid}-fix-ref" class="block text-sm/6 font-medium text-fg">
						Fix task reference
					</Label>
					<Input
						id="{uid}-fix-ref"
						bind:value={fixTaskRef}
						placeholder="T-A40"
						disabled={fixBusy}
						class="mt-1.5 {inputClass}"
					/>
				</div>
			{:else if fixKind === 'commit'}
				<div>
					<Label for="{uid}-fix-commit" class="block text-sm/6 font-medium text-fg">
						Commit SHA
					</Label>
					<Input
						id="{uid}-fix-commit"
						bind:value={fixCommit}
						placeholder="a1b2c3d"
						disabled={fixBusy}
						class="mt-1.5 {inputClass}"
					/>
				</div>
			{/if}

			<div>
				<Label for="{uid}-fix-note" class="block text-sm/6 font-medium text-fg">
					{fixKind === 'note' ? 'Explanation' : 'Note (optional)'}
				</Label>
				<textarea
					id="{uid}-fix-note"
					rows={3}
					bind:value={fixNote}
					disabled={fixBusy}
					class="mt-1.5 {textareaClass}"></textarea>
			</div>

			{#if fixError}
				<p class={errorClass} role="alert">{fixError}</p>
			{/if}

			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={fixBusy}
					onclick={() => (markFixedOpen = false)}>Cancel</Button
				>
				<Button type="submit" class={mobileButton} disabled={fixBusy}>
					{fixBusy ? 'Saving…' : 'Mark as fixed'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

<!-- Verify fix modal (FIX-4, FIX-5) -->
<Dialog bind:open={verifyOpen} onOpenChange={(n) => !n && verifyBusy && (verifyOpen = true)}>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
		<DialogHeader>
			<DialogTitle text="Verify fix for {task.ref}" />
			<DialogDescription>
				Choose whether the fix resolves the bug. "Still happens" requires a note, reopens the bug,
				and increments the reopened count once.
			</DialogDescription>
		</DialogHeader>
		<form class="flex flex-col gap-4" onsubmit={submitVerify} novalidate>
			<div class="flex gap-2" role="radiogroup" aria-label="Verification outcome">
				<label
					class="flex flex-1 cursor-pointer items-center justify-center rounded-md px-3 py-2 text-sm/6 ring-1 ring-inset max-[639px]:min-h-11 {verifyResult ===
					'fixed'
						? 'bg-success-emphasis/10 font-semibold text-success-text ring-success-emphasis/30'
						: 'bg-surface-raised text-fg-secondary ring-line'}"
				>
					<input
						type="radio"
						name="{uid}-verify"
						value="fixed"
						checked={verifyResult === 'fixed'}
						onchange={() => (verifyResult = 'fixed')}
						class="sr-only"
					/>
					Fixed
				</label>
				<label
					class="flex flex-1 cursor-pointer items-center justify-center rounded-md px-3 py-2 text-sm/6 ring-1 ring-inset max-[639px]:min-h-11 {verifyResult ===
					'still_happens'
						? 'bg-danger-emphasis/10 font-semibold text-danger-text ring-danger-emphasis/30'
						: 'bg-surface-raised text-fg-secondary ring-line'}"
				>
					<input
						type="radio"
						name="{uid}-verify"
						value="still_happens"
						checked={verifyResult === 'still_happens'}
						onchange={() => (verifyResult = 'still_happens')}
						class="sr-only"
					/>
					Still happens
				</label>
			</div>

			<div>
				<Label for="{uid}-verify-note" class="block text-sm/6 font-medium text-fg">
					{verifyResult === 'still_happens' ? 'What still happens (required)' : 'Note (optional)'}
				</Label>
				<textarea
					id="{uid}-verify-note"
					rows={3}
					bind:value={verifyNote}
					disabled={verifyBusy}
					aria-invalid={verifyError ? 'true' : undefined}
					class="mt-1.5 {textareaClass}"></textarea>
				{#if verifyError}
					<p class={errorClass} role="alert">{verifyError}</p>
				{/if}
			</div>

			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={verifyBusy}
					onclick={() => (verifyOpen = false)}>Cancel</Button
				>
				<Button type="submit" class={mobileButton} disabled={verifyBusy}>
					{verifyBusy ? 'Saving…' : verifyResult === 'fixed' ? 'Confirm fixed' : 'Reopen bug'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
