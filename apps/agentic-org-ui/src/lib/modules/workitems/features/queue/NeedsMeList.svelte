<!--
  @component
  Work "Show: Needs me" view (work-items-ux §11): work waiting on the owner through available sources
  (owner blocks, review waits, hand-offs, open asks, reached budgets, verifications, critical bugs, approvals).
  Each row links both the work item (/work/[ref]) and its corresponding open Inbox item (/inbox?item=...).
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import { TYPE_WORD, assignmentText, inboxItemHref, taskHref, when } from '../../present';
	import type { NeedsMe, TaskRow } from '../../types';
	import WorkIcon from '../../components/WorkIcon.svelte';
	import StatusBadge from '../../components/StatusBadge.svelte';
	import BudgetChip from '../../components/BudgetChip.svelte';

	let {
		rows,
		sources,
		onopen
	}: {
		rows: TaskRow[];
		sources: NeedsMe['source'][];
		onopen?: (row: TaskRow, triggerEl: HTMLElement | null) => void;
	} = $props();

	function handleOpen(event: MouseEvent, row: TaskRow) {
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
</script>

{#if sources.length === 0}
	<EmptyState
		level={3}
		title="Needs me isn't available from this office yet"
		description="Switch Show to Mine, By project, or By assignee to browse work."
	/>
{:else if rows.length === 0}
	<EmptyState
		level={3}
		title="Nothing needs you in Work"
		description="Owner blocks, verifications, open asks and reached budgets appear here when they wait on you."
	/>
{:else}
	<ul role="list" class="needs-me-list" data-testid="needs-me-list">
		{#each rows as row (row.id)}
			<li class="needs-me-row" data-testid="needs-me-row" data-ref={row.ref}>
				<div class="flex min-w-0 flex-1 flex-col gap-1.5">
					<div class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs/5 text-fg-muted">
						<span class="inline-flex items-center gap-1">
							<WorkIcon name={row.type} class="size-3.5" />
							<span>{TYPE_WORD[row.type]}</span>
							<span class="font-mono text-fg-secondary">{row.ref}</span>
						</span>
						<span>· {row.project ? row.project.name : 'No project'}</span>
						<span>· Accountable {row.team.name}</span>
						<span>· {assignmentText(row)}</span>
					</div>

					<a
						href={resolve(taskHref(row.ref) as '/')}
						onclick={(e) => handleOpen(e, row)}
						class="text-[0.9375rem]/6 font-medium text-fg hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
					>
						{row.title}
					</a>

					{#if row.needsMe}
						<p class="text-sm/6 text-warning-text-strong [overflow-wrap:anywhere]">
							{row.needsMe.text}
						</p>
					{/if}

					<div class="flex flex-wrap items-center gap-2 text-xs/5 text-fg-muted">
						<StatusBadge status={row.status} needsMe={row.needsMe} />
						{#if row.budget}
							<BudgetChip budget={row.budget} hold={row.budgetHold} />
						{/if}
						<span>Updated {when(row.updatedAt)}</span>
					</div>
				</div>

				<div class="flex shrink-0 flex-wrap items-center gap-2">
					{#if row.needsMe?.inboxItemId}
						<a
							href={resolve(inboxItemHref(row.needsMe.inboxItemId) as '/')}
							class="needs-me-btn needs-me-btn-secondary"
							data-testid="needs-me-inbox-link"
						>
							Open Inbox item
						</a>
					{/if}
					<a
						href={resolve(taskHref(row.ref) as '/')}
						onclick={(e) => handleOpen(e, row)}
						class="needs-me-btn needs-me-btn-link"
					>
						Open {row.ref}
					</a>
				</div>
			</li>
		{/each}
	</ul>
{/if}

<style>
	.needs-me-list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
	}
	.needs-me-row {
		display: flex;
		flex-wrap: wrap;
		align-items: flex-start;
		justify-content: space-between;
		gap: 12px 16px;
		padding: 14px 12px;
		border-radius: var(--exec-ctrl-radius, 6px);
		box-shadow: inset 0 -1px var(--exec-border, var(--line-subtle));
		transition: background-color 110ms ease-out;
	}
	.needs-me-row:last-child {
		box-shadow: none;
	}
	.needs-me-row:is(:hover, :focus-within) {
		background: var(
			--work-row-hover,
			var(--triage-row-hover, var(--exec-btn-ghost-bg-hover, var(--surface-raised)))
		);
	}
	.needs-me-btn {
		display: inline-flex;
		align-items: center;
		min-height: 34px;
		padding: 0 12px;
		border-radius: var(--exec-ctrl-radius, 6px);
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
	}
	.needs-me-btn-secondary {
		background: var(--exec-btn-outline-bg, var(--surface-overlay));
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring, var(--line));
		color: var(--exec-btn-outline-ink, var(--fg));
	}
	.needs-me-btn-secondary:hover {
		background: var(--exec-btn-outline-bg-hover, var(--surface-raised));
	}
	.needs-me-btn-link {
		color: var(--tone-accent-ink, var(--link));
	}
	.needs-me-btn-link:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
	}
	.needs-me-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	@media (max-width: 720px), (pointer: coarse) {
		.needs-me-btn {
			min-height: 44px;
		}
	}
</style>
