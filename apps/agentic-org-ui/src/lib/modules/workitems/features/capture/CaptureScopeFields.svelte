<!--
  @component
  Project, accountable team, priority, assigned team, and assignee selectors for CaptureDialog.
-->
<script lang="ts">
	import { Label } from '$lib/components/ui/label';
	import { errorClass, selectClass } from '$lib/components/classes';
	import { PRIORITIES, PRIORITY_WORD } from '../../present';
	import type { Choices, Priority } from '../../types';

	let {
		uid,
		choices,
		choicesUnavailable,
		disabled,
		selectedProject,
		effectiveTeamName,
		teamError,
		chosenProjectId = $bindable(''),
		chosenTeamId = $bindable(''),
		priority = $bindable<Priority>('normal'),
		assignedTeamId = $bindable(''),
		assigneeId = $bindable(''),
		teamSelect = $bindable<HTMLSelectElement | null>(null)
	}: {
		uid: string;
		choices: Choices | null;
		choicesUnavailable: boolean;
		disabled: boolean;
		selectedProject: Choices['projects'][number] | null;
		effectiveTeamName: string;
		teamError?: string;
		chosenProjectId: string;
		chosenTeamId: string;
		priority: Priority;
		assignedTeamId: string;
		assigneeId: string;
		teamSelect: HTMLSelectElement | null;
	} = $props();
</script>

<!-- Project & Accountable team -->
<div class="grid gap-3 sm:grid-cols-2">
	<div>
		<Label for="{uid}-project" class="block text-sm/6 font-medium text-fg">
			Project (optional)
		</Label>
		<select
			id="{uid}-project"
			bind:value={chosenProjectId}
			disabled={disabled || choicesUnavailable}
			class="mt-1.5 {selectClass}"
		>
			<option value="">No project (team work)</option>
			{#each (choices?.projects ?? []).filter((p) => !p.archived) as p (p.id)}
				<option value={p.id}>{p.name} (P-{p.number})</option>
			{/each}
		</select>
	</div>

	<div>
		<Label for="{uid}-team" class="block text-sm/6 font-medium text-fg">Accountable team</Label>
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
				disabled={disabled || choicesUnavailable}
				aria-invalid={teamError ? 'true' : undefined}
				aria-describedby={teamError ? `${uid}-team-err` : undefined}
				class="mt-1.5 {selectClass}"
			>
				<option value="">Choose team…</option>
				{#each choices?.teams ?? [] as t (t.id)}
					<option value={t.id}>{t.path}</option>
				{/each}
			</select>
			{#if teamError}
				<p id="{uid}-team-err" class={errorClass} role="alert">{teamError}</p>
			{/if}
		{/if}
	</div>
</div>

<!-- Priority & Independent Assignment pair -->
<div class="grid gap-3 sm:grid-cols-3">
	<div>
		<Label for="{uid}-priority" class="block text-sm/6 font-medium text-fg">Priority</Label>
		<select id="{uid}-priority" bind:value={priority} {disabled} class="mt-1.5 {selectClass}">
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
			disabled={disabled || choicesUnavailable}
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
			disabled={disabled || choicesUnavailable}
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
