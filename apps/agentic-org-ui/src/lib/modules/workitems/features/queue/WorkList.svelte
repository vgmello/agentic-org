<!--
  @component
  POC-aligned Linear-style divided rows for Tasks, Bugs and Requests
  (work-items-ux §6, §9, §11, §13; feature-requests-ux §2; BUG-10..15, WORK-4, WORK-5, RQS-7).
  Single container-query responsive layout (zero horizontal overflow at 320 px / 200% text).
-->
<script lang="ts">
	import {
		groupRows,
		severityCounts,
		workRowComparator,
		type GroupBy,
		type WorkSortBy,
		type WorkSortDir
	} from '../../present';
	import type { TaskRow, TaskType } from '../../types';
	import WorkListRow from './WorkListRow.svelte';

	let {
		rows,
		tab,
		groupBy,
		subgroupBy = 'none',
		sortBy = 'default',
		sortDir = 'asc',
		showProject = true,
		showDividers = false,
		archivedIds,
		selectedRef = null,
		onopen
	}: {
		rows: TaskRow[];
		tab: TaskType | 'all';
		groupBy: GroupBy;
		subgroupBy?: GroupBy;
		sortBy?: WorkSortBy;
		sortDir?: WorkSortDir;
		showProject?: boolean;
		showDividers?: boolean;
		archivedIds?: ReadonlySet<string>;
		selectedRef?: string | null;
		onopen?: (row: TaskRow, triggerEl: HTMLElement | null) => void;
	} = $props();

	const uid = $props.id();
	const sortFn = $derived(workRowComparator(tab, sortBy, sortDir));
	const groups = $derived(groupRows(rows, groupBy, sortFn));
	const effectiveSubgroupBy = $derived<GroupBy>(
		groupBy !== 'none' && subgroupBy !== 'none' && subgroupBy !== groupBy ? subgroupBy : 'none'
	);
</script>

<div class="work-items" data-testid="work-list" data-work-dividers={showDividers ? '' : undefined}>
	{#each groups as group (group.key)}
		{@const headingId = `${uid}-group-${group.key || 'none'}`}
		{@const subgroups =
			effectiveSubgroupBy !== 'none'
				? groupRows(group.rows, effectiveSubgroupBy, sortFn)
				: [{ key: 'all', label: '', rows: group.rows }]}
		<section aria-labelledby={headingId} class="work-group">
			{#if groupBy !== 'none'}
				<div class="work-group-title" data-testid="work-group-heading">
					<h3 id={headingId} class="work-group-label">
						<span>{group.label}</span>
						<span class="cnt">{group.rows.length}</span>
					</h3>
					{#if tab === 'bug'}
						<p class="work-group-meta" data-testid="group-severity-counts">
							{severityCounts(group.rows)}
						</p>
					{/if}
				</div>
			{:else}
				<h3 id={headingId} class="sr-only">Work items</h3>
			{/if}

			<div class={effectiveSubgroupBy !== 'none' ? 'work-subgroups' : undefined}>
				{#each subgroups as subgroup (subgroup.key)}
					<div class={effectiveSubgroupBy !== 'none' ? 'work-subgroup' : undefined}>
						{#if effectiveSubgroupBy !== 'none'}
							<h4 class="work-subgroup-title" data-testid="work-subgroup-heading">
								<span>{subgroup.label}</span>
								<span class="cnt">{subgroup.rows.length}</span>
							</h4>
						{/if}

						<ul role="list" class="work-list">
							{#each subgroup.rows as row (row.id)}
								<WorkListRow
									{row}
									{tab}
									{showProject}
									{showDividers}
									archived={Boolean(archivedIds?.has(row.id))}
									selected={selectedRef === row.ref}
									{onopen}
								/>
							{/each}
						</ul>
					</div>
				{/each}
			</div>
		</section>
	{/each}
</div>

<style>
	.work-items {
		container-type: inline-size;
		display: flex;
		flex-direction: column;
		min-width: 0;
	}
	.work-group + .work-group {
		margin-top: 12px;
	}
	.work-subgroups {
		margin-inline-start: 8px;
	}
	.work-subgroup + .work-subgroup {
		margin-block-start: 6px;
	}
	.work-group-title,
	.work-subgroup-title {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		justify-content: space-between;
		gap: 8px;
		padding: 6px 12px;
		font-size: 0.8125rem;
		font-weight: 600;
		color: var(--exec-ink, var(--fg));
	}
	.work-group-title {
		border-radius: var(--exec-ctrl-radius, 6px);
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line-subtle));
		margin-bottom: 4px;
	}
	.work-group-label {
		display: inline-flex;
		align-items: baseline;
		gap: 8px;
		font-size: inherit;
		font-weight: inherit;
		color: inherit;
		margin: 0;
	}
	.work-group-meta {
		margin: 0;
		font-size: 0.75rem;
		font-weight: 400;
		color: var(--exec-muted, var(--fg-muted));
	}
	.work-subgroup-title {
		justify-content: flex-start;
		color: var(--exec-body, var(--fg-secondary));
		padding-block: 5px 3px;
		margin: 0;
	}
	.work-group-title .cnt,
	.work-subgroup-title .cnt {
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.75rem;
		font-weight: 500;
		font-variant-numeric: tabular-nums;
	}
	.work-list {
		list-style: none;
		margin: 0;
		padding: 0;
	}

	@media (forced-colors: active) {
		.work-group-title {
			border: 1px solid CanvasText;
		}
	}
</style>
