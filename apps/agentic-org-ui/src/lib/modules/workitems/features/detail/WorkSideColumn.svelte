<!--
  @component
  Side column on /work/[ref] (work-items-ux §5, §7.1, §9; TRI-2/3/6, TSK-8, TSK-13):
  Metadata description list, independent atomic Assignment pair editor with conflict resolution,
  immutable reporter, linked tasks, per-repository branch/worktree/commit evidence, and Cost and time block.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import { toAppError } from '$lib/proto';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import { errorClass, mobileButton, selectClass } from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import {
		PRIORITIES,
		PRIORITY_WORD,
		RESOLUTION_WORD,
		STATUS_WORD,
		TYPE_WORD,
		assignmentText,
		linkText,
		memberHref,
		projectHref,
		severityWord,
		taskHref,
		teamHref,
		when
	} from '../../present';
	import type { AssignmentConflict, Choices, Priority, TaskDetail } from '../../types';
	import WorkIcon from '../../components/WorkIcon.svelte';
	import TaskCostBlock from '../budgets/TaskCostBlock.svelte';

	let {
		organizationId,
		task,
		choices,
		stale = false,
		onchanged
	}: {
		organizationId: string;
		task: TaskDetail;
		choices: Choices | null;
		stale?: boolean;
		onchanged: () => void;
	} = $props();

	const uid = $props.id();

	// Atomic assignment pair editor state (§5, TRI-6, TSK-13)
	let assignedTeamId = $derived(task.assignedTeam?.id ?? '');
	let assigneeId = $derived(task.assignee?.id ?? '');
	let assignBusy = $state(false);
	let assignError = $state<string | undefined>();
	let assignConflict = $state<AssignmentConflict | null>(null);
	let priorityBusy = $state(false);

	const assignDirty = $derived(
		assignedTeamId !== (task.assignedTeam?.id ?? '') || assigneeId !== (task.assignee?.id ?? '')
	);
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

	async function saveAssignment(expectedRevision: number) {
		if (assignBusy || stale) return;
		assignBusy = true;
		assignError = undefined;
		try {
			await workItemsApi.assign(organizationId, task.id, {
				assignedTeamId: assignedTeamId || null,
				assigneeId: assigneeId || null,
				expectedRevision
			});
			assignConflict = null;
			toast.success(`Updated assignment on ${task.ref}.`);
			onchanged();
		} catch (e) {
			const raw = e as { code?: string; current?: AssignmentConflict };
			if (raw?.code === 'conflict' && raw.current) {
				assignConflict = raw.current;
			} else {
				assignError = toAppError(e).message;
			}
		} finally {
			assignBusy = false;
		}
	}

	async function changePriority(selectEl: HTMLSelectElement) {
		const priority = selectEl.value as Priority;
		if (priorityBusy || stale || priority === task.priority) return;
		priorityBusy = true;
		try {
			await workItemsApi.setPriority(organizationId, task.id, {
				priority,
				expectedRevision: task.revision
			});
			toast.success(`Priority set to ${PRIORITY_WORD[priority]}.`);
			onchanged();
		} catch (e) {
			selectEl.value = task.priority;
			toast.error(toAppError(e).message);
		} finally {
			priorityBusy = false;
		}
	}
</script>

<aside aria-label="Details" class="flex flex-col gap-4" data-testid="work-side-column">
	<!-- Core metadata description list -->
	<section
		aria-labelledby="{uid}-details-heading"
		class="flex flex-col gap-3 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
	>
		<h3 id="{uid}-details-heading" class="text-sm/6 font-semibold text-fg">Details</h3>

		<dl class="divide-y divide-line-subtle text-sm/6">
			<div class="flex items-baseline justify-between gap-3 py-2">
				<dt class="text-xs/5 text-fg-muted">Reference</dt>
				<dd class="inline-flex items-center gap-1.5 font-mono text-xs/5 text-fg">
					<WorkIcon name={task.type} class="size-3.5 text-fg-muted" />
					<span>{TYPE_WORD[task.type]} {task.ref}</span>
				</dd>
			</div>

			<div class="flex items-baseline justify-between gap-3 py-2">
				<dt class="text-xs/5 text-fg-muted">Accountable team</dt>
				<dd>
					<a
						href={resolve(teamHref(task.team.id) as '/')}
						class="text-sm/6 font-medium text-link hover:underline"
					>
						{task.team.name}
					</a>
				</dd>
			</div>

			<div class="flex items-baseline justify-between gap-3 py-2">
				<dt class="text-xs/5 text-fg-muted">Project</dt>
				<dd>
					{#if task.project}
						<a
							href={resolve(projectHref(task.project.id) as '/')}
							class="text-sm/6 font-medium text-link hover:underline"
						>
							{task.project.name}
							<span class="font-mono text-xs/5 text-fg-muted">P-{task.project.number}</span>
						</a>
					{:else}
						<span class="text-fg-muted">No project</span>
					{/if}
				</dd>
			</div>

			{#if task.feature}
				<div class="flex items-baseline justify-between gap-3 py-2">
					<dt class="text-xs/5 text-fg-muted">Feature</dt>
					<dd class="text-right text-fg-secondary">
						<span class="font-mono text-xs/5 text-fg-muted">{task.feature.ref}</span>
						{task.feature.title}
					</dd>
				</div>
			{/if}

			{#if task.milestone}
				<div class="flex items-baseline justify-between gap-3 py-2">
					<dt class="text-xs/5 text-fg-muted">Milestone</dt>
					<dd class="text-fg-secondary">{task.milestone.name}</dd>
				</div>
			{/if}

			{#if task.type === 'bug'}
				<div class="flex items-baseline justify-between gap-3 py-2">
					<dt class="text-xs/5 text-fg-muted">Severity</dt>
					<dd
						class="inline-flex items-center gap-1 text-xs/5 {task.severity === 'critical'
							? 'text-danger-text'
							: 'text-fg-secondary'}"
					>
						<WorkIcon name={task.severity ?? 'unset'} class="size-3.5" />
						<span>{severityWord(task.severity)}</span>
					</dd>
				</div>
			{/if}

			<div class="flex items-center justify-between gap-3 py-2">
				<dt class="text-xs/5 text-fg-muted">
					<Label for="{uid}-side-priority" class="text-xs/5 font-normal text-fg-muted">
						Priority
					</Label>
				</dt>
				<dd>
					{#if task.can.edit && !stale}
						<select
							id="{uid}-side-priority"
							value={task.priority}
							disabled={priorityBusy}
							onchange={(e) => changePriority(e.currentTarget as HTMLSelectElement)}
							class="w-auto {selectClass}"
						>
							{#each PRIORITIES as p (p)}
								<option value={p}>{PRIORITY_WORD[p]}</option>
							{/each}
						</select>
					{:else}
						<span class="inline-flex items-center gap-1 text-xs/5 text-fg-secondary">
							<WorkIcon name={task.priority} class="size-3.5 text-fg-muted" />
							<span>{PRIORITY_WORD[task.priority]}</span>
						</span>
					{/if}
				</dd>
			</div>

			{#if task.effort !== null}
				<div class="flex items-baseline justify-between gap-3 py-2">
					<dt class="text-xs/5 text-fg-muted">Effort</dt>
					<dd class="text-fg-secondary tabular-nums">{task.effort} points</dd>
				</div>
			{/if}

			{#if task.resolution}
				<div class="flex items-baseline justify-between gap-3 py-2">
					<dt class="text-xs/5 text-fg-muted">Resolution</dt>
					<dd class="text-fg-secondary">{RESOLUTION_WORD[task.resolution]}</dd>
				</div>
			{/if}

			<div class="flex items-baseline justify-between gap-3 py-2">
				<dt class="text-xs/5 text-fg-muted">Reporter</dt>
				<dd class="text-right text-xs/5 text-fg-secondary" data-testid="immutable-reporter">
					<a href={resolve(memberHref(task.reporter.id) as '/')} class="text-link hover:underline">
						{task.reporter.name}
					</a>
					<span class="ml-1 text-fg-muted">{task.reporter.kind}</span>
					{#if task.reporter.former}
						<span class="ml-1 text-fg-muted">(former)</span>
					{/if}
					<div class="text-fg-muted">{when(task.createdAt)}</div>
				</dd>
			</div>

			<div class="flex items-baseline justify-between gap-3 py-2">
				<dt class="text-xs/5 text-fg-muted">Latest update</dt>
				<dd class="text-right text-xs/5 text-fg-muted">
					{task.latestUpdate.by.name} · {when(task.latestUpdate.at)}
				</dd>
			</div>
		</dl>
	</section>

	<!-- Atomic assignment pair editor (§5, TRI-2/3/6, TSK-13) -->
	<section
		aria-labelledby="{uid}-assign-heading"
		class="flex flex-col gap-3 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
		data-testid="assignment-section"
	>
		<h3 id="{uid}-assign-heading" class="text-sm/6 font-semibold text-fg">Assignment</h3>

		<div class="flex flex-col gap-2.5">
			<div>
				<Label for="{uid}-assign-team" class="block text-xs/5 font-medium text-fg-secondary">
					Assigned team
				</Label>
				<select
					id="{uid}-assign-team"
					bind:value={assignedTeamId}
					disabled={!task.can.edit || stale || assignBusy}
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
				<Label for="{uid}-assign-person" class="block text-xs/5 font-medium text-fg-secondary">
					Assignee
				</Label>
				<select
					id="{uid}-assign-person"
					bind:value={assigneeId}
					disabled={!task.can.edit || stale || assignBusy}
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
						<option value={emp.id}>
							{emp.name} ({emp.kind}{emp.availability ? ` · ${emp.availability}` : ''})
						</option>
					{/each}
				</select>
			</div>

			{#if assignConflict}
				<div
					class="flex flex-col gap-2 rounded-lg bg-danger-emphasis/10 p-3 text-xs/5 text-danger-text ring-1 ring-danger-emphasis/20"
					role="alert"
					data-testid="assignment-conflict"
				>
					<p class="font-semibold">
						Assignment changed by {assignConflict.by.name} · {when(assignConflict.at)}.
					</p>
					<p class="text-fg-secondary">
						Current: {assignmentText(assignConflict)} (rev {assignConflict.revision}). Your desired
						pair is kept above.
					</p>
					<div class="flex flex-wrap gap-2">
						<Button
							type="button"
							size="sm"
							class={mobileButton}
							disabled={assignBusy}
							onclick={() => saveAssignment(assignConflict!.revision)}
						>
							Retry at rev {assignConflict.revision}
						</Button>
						<Button
							type="button"
							variant="outline"
							size="sm"
							class={mobileButton}
							onclick={() => ((assignConflict = null), onchanged())}
						>
							Keep current
						</Button>
					</div>
				</div>
			{:else}
				{#if assignError}
					<p class={errorClass} role="alert">{assignError}</p>
				{/if}
				{#if task.can.edit && !stale}
					<Button
						type="button"
						variant="outline"
						size="sm"
						class={mobileButton}
						disabled={!assignDirty || assignBusy}
						onclick={() => saveAssignment(task.revision)}
					>
						{assignBusy ? 'Saving assignment…' : 'Save assignment'}
					</Button>
				{/if}
			{/if}
		</div>
	</section>

	<!-- Cost and time + Budgets -->
	<TaskCostBlock
		{organizationId}
		taskId={task.id}
		taskRef={task.ref}
		taskType={task.type}
		time={task.time}
		{stale}
		ontaskchanged={onchanged}
	/>

	<!-- Linked tasks (fixes / relatesTo) -->
	{#if task.fixes.length > 0 || task.relatesTo.length > 0}
		<section
			aria-labelledby="{uid}-links-heading"
			class="flex flex-col gap-2 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
		>
			<h3 id="{uid}-links-heading" class="text-sm/6 font-semibold text-fg">Linked work</h3>
			{#if task.fixes.length > 0}
				<div>
					<p class="text-xs/5 text-fg-muted">Fixes</p>
					<ul role="list" class="mt-1 flex flex-col gap-1 text-xs/5">
						{#each task.fixes as link (link.id)}
							<li class="flex items-center justify-between gap-2">
								<a
									href={resolve(taskHref(link.ref) as '/')}
									class="text-link hover:underline [overflow-wrap:anywhere]"
								>
									{linkText(link)}
								</a>
								<span class="shrink-0 text-fg-muted">{STATUS_WORD[link.status]}</span>
							</li>
						{/each}
					</ul>
				</div>
			{/if}
			{#if task.relatesTo.length > 0}
				<div>
					<p class="text-xs/5 text-fg-muted">Related</p>
					<ul role="list" class="mt-1 flex flex-col gap-1 text-xs/5">
						{#each task.relatesTo as link (link.id)}
							<li class="flex items-center justify-between gap-2">
								<a
									href={resolve(taskHref(link.ref) as '/')}
									class="text-link hover:underline [overflow-wrap:anywhere]"
								>
									{linkText(link)}
								</a>
								<span class="shrink-0 text-fg-muted">{STATUS_WORD[link.status]}</span>
							</li>
						{/each}
					</ul>
				</div>
			{/if}
		</section>
	{/if}

	<!-- Per-repository evidence (TSK-8) -->
	{#if task.repositories.length > 0}
		<section
			aria-labelledby="{uid}-repos-heading"
			class="flex flex-col gap-2 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
			data-testid="repository-evidence"
		>
			<h3 id="{uid}-repos-heading" class="text-sm/6 font-semibold text-fg">Repository evidence</h3>
			<ul role="list" class="flex flex-col gap-2 text-xs/5">
				{#each task.repositories as repo (repo.repository)}
					<li class="rounded-md bg-surface p-2.5 outline-1 -outline-offset-1 outline-line-subtle">
						<p class="font-mono font-medium text-fg">{repo.repository}</p>
						{#if repo.branch}
							<p class="text-fg-secondary">
								Branch: <span class="font-mono">{repo.branch}</span>
							</p>
						{/if}
						{#if repo.worktree}
							<p class="text-fg-muted [overflow-wrap:anywhere]">
								Worktree: <span class="font-mono">{repo.worktree}</span>
							</p>
						{/if}
						{#if repo.integratedCommit}
							<p class="text-fg-secondary">
								Commit: <span class="font-mono">{repo.integratedCommit}</span>
							</p>
						{/if}
					</li>
				{/each}
			</ul>
		</section>
	{/if}
</aside>
