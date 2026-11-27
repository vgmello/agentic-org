<!--
  @component
  Single Linear-style work item row inside WorkList (BUG-10..15, WORK-4, WORK-5, RQS-7).
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import { projectDotTone } from '$lib/modules/work/compact';
	import { TYPE_WORD, ageOf, rowSpeech, severityWord, taskHref } from '../../present';
	import type { TaskRow, TaskType } from '../../types';
	import BudgetChip from '../../components/BudgetChip.svelte';
	import WorkStatusIcon from './WorkStatusIcon.svelte';

	let {
		row,
		tab,
		showProject = true,
		showDividers = false,
		archived = false,
		selected = false,
		onopen
	}: {
		row: TaskRow;
		tab: TaskType | 'all';
		showProject?: boolean;
		showDividers?: boolean;
		archived?: boolean;
		selected?: boolean;
		onopen?: (row: TaskRow, triggerEl: HTMLElement | null) => void;
	} = $props();

	function handleOpen(event: MouseEvent) {
		if (
			!onopen ||
			event.defaultPrevented ||
			event.button !== 0 ||
			event.metaKey ||
			event.ctrlKey ||
			event.shiftKey ||
			event.altKey
		) {
			return;
		}
		event.preventDefault();
		onopen(row, event.currentTarget as HTMLElement | null);
	}

	const teamChip = $derived(row.assignedTeam ?? row.team);
	const teamDot = $derived(projectDotTone(teamChip.id));
</script>

<li
	class="work-row"
	data-testid="work-row"
	data-ref={row.ref}
	data-work-divider={showDividers ? '' : undefined}
	aria-current={selected ? 'true' : undefined}
>
	<div class="work-summary">
		<div class="work-ident">
			<WorkStatusIcon variant="priority" priority={row.priority} />
			<span class="work-ref">{row.ref}</span>
		</div>

		<span class="work-markers">
			<WorkStatusIcon variant="status" status={row.status} needsMe={Boolean(row.needsMe)} />

			{#if row.checklist.total > 0}
				<span
					class="work-criteria"
					data-done={row.checklist.done === row.checklist.total ? '' : undefined}
					title="{row.checklist.done} of {row.checklist.total} steps done"
				>
					{row.checklist.done}/{row.checklist.total}
				</span>
			{:else}
				<span class="work-criteria is-empty" aria-hidden="true">–</span>
			{/if}
		</span>

		<div class="work-heading">
			<a
				href={resolve(taskHref(row.ref) as '/')}
				aria-label={rowSpeech(row)}
				onclick={handleOpen}
				class="work-title"
			>
				{row.title}
			</a>

			{#if tab === 'all' && row.type !== 'task'}
				<span class="work-inline-chip">{TYPE_WORD[row.type]}</span>
			{/if}

			{#if row.type === 'bug'}
				<span class="work-inline-chip" data-tone={row.severity === 'critical' ? 'bad' : undefined}>
					{severityWord(row.severity)}
				</span>
			{/if}

			{#if row.needsMe}
				<span class="work-inline-chip" data-tone="warn">Needs you</span>
			{/if}

			{#if row.awaitingVerification}
				<span class="work-inline-chip">Verify fix</span>
			{/if}

			{#if archived}
				<span class="work-inline-chip work-archived-tag" data-testid="work-archived-tag"
					>Archived</span
				>
			{/if}

			{#if row.budget}
				<BudgetChip budget={row.budget} hold={row.budgetHold} />
			{/if}
		</div>
	</div>

	<div class="work-facts">
		{#if showProject}
			<span
				class="work-project"
				data-muted={!row.project ? '' : undefined}
				data-testid="work-project"
			>
				{row.project ? row.project.name : 'No project'}
			</span>
		{/if}

		<span
			class="work-team"
			title={row.assignedTeam
				? `Assigned team: ${row.assignedTeam.name}`
				: `Accountable team: ${row.team.name}`}
		>
			<i class="work-team-dot" data-dot={teamDot} aria-hidden="true"></i>
			<span>{teamChip.name}</span>
		</span>

		<span class="work-owner" data-unassigned={!row.assignee ? '' : undefined}>
			{#if row.assignee}
				<Avatar name={row.assignee.name} size="sm" />
				<span class="work-owner-name">{row.assignee.name}</span>
			{:else}
				<span class="work-owner-name">Unassigned</span>
			{/if}
		</span>

		<span class="work-age" title="Created {ageOf(row.createdAt)} ago">
			{ageOf(row.createdAt)}
		</span>
	</div>
</li>

<style>
	.work-row {
		position: relative;
		display: grid;
		grid-template-columns: minmax(0, 1fr) auto;
		align-items: center;
		gap: 6px 16px;
		min-height: 44px;
		padding: 9px 12px;
		border-radius: var(--exec-ctrl-radius, 6px);
		transition: background-color 110ms ease-out;
	}
	.work-row:is(:hover, :focus-within) {
		background: var(
			--work-row-hover,
			var(--triage-row-hover, var(--exec-btn-ghost-bg-hover, var(--surface-raised)))
		);
	}
	.work-row[aria-current='true'] {
		background: var(--work-row-selected, var(--triage-row-selected, var(--tone-accent-bg)));
		box-shadow: inset 3px 0 0 var(--work-selected-bar, var(--tone-accent-ink));
	}
	.work-row[data-work-divider]:not([aria-current='true']) {
		box-shadow: inset 0 -1px var(--exec-border, var(--line-subtle));
	}
	.work-row[data-work-divider]:last-child:not([aria-current='true']),
	.work-row[data-work-divider]:is(:hover, :focus-within):not([aria-current='true']) {
		box-shadow: none;
	}
	.work-summary {
		display: flex;
		align-items: flex-start;
		gap: 10px;
		min-width: 0;
	}
	.work-ident,
	.work-markers {
		display: inline-flex;
		flex: none;
		align-items: center;
		gap: 6px;
		min-height: 1.4375rem;
	}
	.work-ref {
		color: var(--exec-muted, var(--fg-muted));
		font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, monospace);
		font-size: 0.78125rem;
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
		min-width: 3.25rem;
	}
	.work-heading {
		display: flex;
		flex: 1 1 auto;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px 8px;
		min-width: 0;
	}
	.work-criteria {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: 2.5rem;
		padding: 1px 6px;
		border-radius: var(--exec-sm-radius, 4px);
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line-subtle));
		color: var(--exec-muted, var(--fg-muted));
		font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, monospace);
		font-size: 0.6875rem;
		line-height: 1.35;
		font-variant-numeric: tabular-nums;
	}
	.work-criteria.is-empty {
		background: transparent;
		box-shadow: none;
		color: var(--exec-dim, var(--fg-subtle));
		opacity: 0.55;
	}
	.work-criteria[data-done] {
		color: var(--tone-ok-ink, var(--success-text));
		background: var(--tone-ok-bg, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--tone-ok-ring, var(--line-subtle));
	}
	.work-title {
		color: var(--exec-ink, var(--fg));
		font-size: 0.9375rem;
		font-weight: 550;
		line-height: 1.4;
		letter-spacing: -0.005em;
		text-decoration: none;
		overflow-wrap: anywhere;
		min-width: 0;
	}
	.work-title:hover {
		text-decoration: underline;
		text-underline-offset: 2px;
	}
	.work-title:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
		border-radius: 3px;
	}
	.work-inline-chip {
		display: inline-flex;
		align-items: center;
		padding: 1px 6px;
		border-radius: var(--exec-sm-radius, 4px);
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line-subtle));
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.71875rem;
		font-weight: 500;
		line-height: 1.35;
		white-space: nowrap;
	}
	.work-inline-chip[data-tone='warn'] {
		background: var(--tone-warn-bg, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--tone-warn-ring, var(--line));
		color: var(--tone-warn-ink, var(--warning-text));
	}
	.work-inline-chip[data-tone='bad'] {
		background: var(--tone-bad-bg, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--tone-bad-ring, var(--line));
		color: var(--tone-bad-ink, var(--danger-text));
	}
	.work-facts {
		display: inline-flex;
		align-items: center;
		gap: 12px;
		min-width: 0;
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.8125rem;
		white-space: nowrap;
	}
	.work-project {
		color: var(--exec-body, var(--fg-secondary));
		max-width: 11rem;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.work-project[data-muted] {
		color: var(--exec-dim, var(--fg-subtle));
	}
	.work-team {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 2px 8px;
		border-radius: 999px;
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line-subtle));
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.75rem;
		font-weight: 500;
		max-width: 10rem;
	}
	.work-team span,
	.work-owner-name {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.work-team-dot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		flex: none;
		background: var(--tone-accent, var(--brand));
	}
	.work-team-dot[data-dot='ok'] {
		background: var(--tone-ok, var(--success));
	}
	.work-team-dot[data-dot='warn'] {
		background: var(--tone-warn, var(--warning));
	}
	.work-team-dot[data-dot='queued'] {
		background: var(--tone-queued, var(--brand));
	}
	.work-team-dot[data-dot='bad'] {
		background: var(--tone-bad, var(--danger));
	}
	.work-owner {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		min-width: 7.5rem;
		max-width: 10.5rem;
		color: var(--exec-body, var(--fg-secondary));
	}
	.work-owner[data-unassigned] {
		color: var(--exec-dim, var(--fg-subtle));
	}
	.work-age {
		color: var(--exec-dim, var(--fg-subtle));
		font-size: 0.75rem;
		font-variant-numeric: tabular-nums;
		min-width: 2.75rem;
		text-align: right;
	}

	@container (max-width: 56rem) {
		.work-row {
			grid-template-columns: minmax(0, 1fr);
			gap: 5px;
			padding-block: 10px;
		}
		.work-facts {
			flex-wrap: wrap;
			gap: 6px 10px;
			padding-inline-start: 4.9rem;
			white-space: normal;
		}
		.work-owner,
		.work-age {
			min-width: 0;
		}
		.work-age {
			text-align: left;
		}
	}

	@container (max-width: 34rem) {
		.work-summary {
			flex-direction: column;
			gap: 3px;
		}
		.work-facts {
			padding-inline-start: 0;
		}
		.work-project,
		.work-team,
		.work-owner {
			max-width: 100%;
		}
	}

	@media (forced-colors: active) {
		.work-row[aria-current='true'] {
			outline: 2px solid Highlight;
		}
	}
</style>
