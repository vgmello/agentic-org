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
		sources
	}: {
		rows: TaskRow[];
		sources: NeedsMe['source'][];
	} = $props();
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
	<ul
		role="list"
		class="divide-y divide-line-subtle rounded-xl bg-surface-raised outline-1 -outline-offset-1 outline-line"
		data-testid="needs-me-list"
	>
		{#each rows as row (row.id)}
			<li
				class="flex flex-wrap items-start justify-between gap-4 px-4 py-4 min-[640px]:px-6"
				data-testid="needs-me-row"
				data-ref={row.ref}
			>
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
						class="text-sm/6 font-semibold text-fg hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
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

				<div class="flex shrink-0 flex-wrap items-center gap-3">
					{#if row.needsMe?.inboxItemId}
						<a
							href={resolve(inboxItemHref(row.needsMe.inboxItemId) as '/')}
							class="inline-flex items-center rounded-md bg-surface-overlay px-3 py-1.5 text-sm/6 font-medium text-fg ring-1 ring-line hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
							data-testid="needs-me-inbox-link"
						>
							Open Inbox item
						</a>
					{/if}
					<a
						href={resolve(taskHref(row.ref) as '/')}
						class="inline-flex items-center rounded-md px-3 py-1.5 text-sm/6 font-medium text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
					>
						Open {row.ref}
					</a>
				</div>
			</li>
		{/each}
	</ul>
{/if}
