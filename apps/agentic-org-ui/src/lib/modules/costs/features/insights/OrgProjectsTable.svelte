<!--
  @component
  Sortable, filterable Projects investment table (desktop table + mobile list) for OrganizationInsightsView.
-->
<script lang="ts">
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ArrowUpDownIcon from '@lucide/svelte/icons/arrow-up-down';
	import { resolve } from '$app/paths';
	import { gutters, inputClass } from '$lib/components/classes';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { WORKING_TIME_NOT_TRACKED } from '../../duration';
	import {
		filterAndSortOrgProjects,
		formatChangePercent,
		type OrgProjectSortKey,
		type SortDir
	} from '../../insights-helpers';
	import { formatCost, formatCount } from '../../money';
	import { incompleteText } from '../../present';
	import type { OrgInsightProjectRow, UsageTotals } from '../../types';

	let {
		projects,
		notOnTask,
		onOpenProjectRecords,
		onOpenNotOnTaskRecords
	}: {
		projects: readonly OrgInsightProjectRow[];
		notOnTask: UsageTotals | undefined;
		onOpenProjectRecords: (row: OrgInsightProjectRow, from: HTMLElement) => void;
		onOpenNotOnTaskRecords: (from: HTMLElement) => void;
	} = $props();

	const filterId = $props.id();
	let query = $state('');
	let includeArchived = $state(false);
	let sortKey = $state<OrgProjectSortKey>('toDate');
	let sortDir = $state<SortDir>('desc');

	const visibleProjects = $derived(
		filterAndSortOrgProjects(projects, query, includeArchived, sortKey, sortDir)
	);
	const hasArchived = $derived(projects.some((p) => p.archived));
	const showNotOnTask = $derived.by(() => {
		if (!notOnTask) return false;
		const needle = query.trim().toLowerCase();
		if (!needle) return true;
		return 'not on a task'.includes(needle) || 'overhead'.includes(needle);
	});

	function toggleSort(key: OrgProjectSortKey) {
		if (sortKey === key) {
			sortDir = sortDir === 'desc' ? 'asc' : 'desc';
		} else {
			sortKey = key;
			sortDir = key === 'name' ? 'asc' : 'desc';
		}
	}

	function ariaSort(key: OrgProjectSortKey): 'ascending' | 'descending' | 'none' {
		if (sortKey !== key) return 'none';
		return sortDir === 'asc' ? 'ascending' : 'descending';
	}

	const sortBtnClass =
		'inline-flex items-center gap-1 rounded-sm font-semibold text-fg-muted hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus';
	const figureClass =
		'inline-flex min-h-6 items-center rounded-sm font-semibold text-link tabular-nums hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11';
</script>

{#snippet sortIcon(key: OrgProjectSortKey)}
	{#if sortKey === key && sortDir === 'asc'}
		<ArrowUpIcon class="size-3.5" aria-hidden="true" />
	{:else if sortKey === key && sortDir === 'desc'}
		<ArrowDownIcon class="size-3.5" aria-hidden="true" />
	{:else}
		<ArrowUpDownIcon class="size-3.5 opacity-60" aria-hidden="true" />
	{/if}
{/snippet}

<section
	aria-labelledby="org-projects-heading"
	class="border-t border-line-subtle py-6"
	data-testid="org-projects-section"
>
	<div class="flex flex-wrap items-end justify-between gap-4 {gutters}">
		<div>
			<h3 id="org-projects-heading" class="text-base/7 font-semibold text-fg">Projects</h3>
			<p class="mt-1 text-sm/6 text-fg-muted">
				Investment to date, this month vs last month, and items done. Select a project to open its
				Insights.
			</p>
		</div>
		<div class="flex flex-wrap items-center gap-3">
			{#if hasArchived}
				<label class="inline-flex min-h-9 items-center gap-2 text-sm/6 text-fg-secondary">
					<input
						type="checkbox"
						bind:checked={includeArchived}
						class="size-4 rounded border-line bg-surface-raised accent-brand"
					/>
					Show archived
				</label>
			{/if}
			<div class="w-full min-w-0 min-[640px]:w-64">
				<Label for={filterId} class="sr-only">Filter projects</Label>
				<Input
					id={filterId}
					type="search"
					bind:value={query}
					placeholder="Filter projects or team…"
					class="w-full {inputClass}"
					data-testid="org-projects-filter"
				/>
			</div>
		</div>
	</div>

	<div class="mt-4 @container">
		{#if visibleProjects.length === 0 && !showNotOnTask}
			<p class="py-8 text-sm/6 text-fg-muted {gutters}" data-testid="org-projects-empty">
				{query.trim()
					? `No projects match "${query.trim()}".`
					: 'No projects with visible cost records.'}
			</p>
		{:else}
			<table class="hidden w-full text-sm/6 @4xl:table" data-testid="org-projects-table">
				<caption class="sr-only">Projects investment and cost overview</caption>
				<thead>
					<tr class="border-b border-line text-left text-xs/5 font-semibold text-fg-muted">
						<th
							scope="col"
							aria-sort={ariaSort('name')}
							class="py-3 pr-3 pl-4 min-[640px]:pl-6 min-[1024px]:pl-8"
						>
							<button type="button" class={sortBtnClass} onclick={() => toggleSort('name')}>
								Project
								{@render sortIcon('name')}
							</button>
						</th>
						<th scope="col" aria-sort={ariaSort('toDate')} class="px-3 py-3 text-right">
							<button
								type="button"
								class="{sortBtnClass} ml-auto"
								onclick={() => toggleSort('toDate')}
							>
								Investment to date
								{@render sortIcon('toDate')}
							</button>
						</th>
						<th scope="col" aria-sort={ariaSort('thisMonth')} class="px-3 py-3 text-right">
							<button
								type="button"
								class="{sortBtnClass} ml-auto"
								onclick={() => toggleSort('thisMonth')}
							>
								This month
								{@render sortIcon('thisMonth')}
							</button>
						</th>
						<th scope="col" aria-sort={ariaSort('lastMonth')} class="px-3 py-3 text-right">
							<button
								type="button"
								class="{sortBtnClass} ml-auto"
								onclick={() => toggleSort('lastMonth')}
							>
								Last month
								{@render sortIcon('lastMonth')}
							</button>
						</th>
						<th scope="col" aria-sort={ariaSort('change')} class="px-3 py-3 text-right">
							<button
								type="button"
								class="{sortBtnClass} ml-auto"
								onclick={() => toggleSort('change')}
							>
								Change
								{@render sortIcon('change')}
							</button>
						</th>
						<th scope="col" class="px-3 py-3 text-right">Working time</th>
						<th
							scope="col"
							aria-sort={ariaSort('itemsDone')}
							class="py-3 pr-4 pl-3 text-right min-[640px]:pr-6 min-[1024px]:pr-8"
						>
							<button
								type="button"
								class="{sortBtnClass} ml-auto"
								onclick={() => toggleSort('itemsDone')}
							>
								Items done
								{@render sortIcon('itemsDone')}
							</button>
						</th>
					</tr>
				</thead>
				<tbody>
					{#each visibleProjects as proj (proj.projectId)}
						{@const rowInc = incompleteText(proj.toDate)}
						<tr class="border-b border-line-subtle align-top" data-org-project={proj.projectId}>
							<th
								scope="row"
								class="py-4 pr-3 pl-4 text-left font-normal min-[640px]:pl-6 min-[1024px]:pl-8"
							>
								<a
									href={resolve(`/projects/${encodeURIComponent(proj.projectId)}/insights` as '/')}
									class="rounded-sm font-semibold text-link hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
									>{proj.name}</a
								>
								<span class="ml-1.5 font-mono text-xs/5 text-fg-muted">P-{proj.projectNumber}</span>
								{#if proj.accountableTeam}
									<span class="ml-1.5 text-xs/5 text-fg-muted">· {proj.accountableTeam}</span>
								{/if}
								{#if proj.archived}
									<span class="ml-1.5 text-xs/5 text-fg-muted">archived</span>
								{/if}
								{#if rowInc}
									<p class="mt-1 text-xs/5 text-fg-muted">{rowInc}</p>
								{/if}
							</th>
							<td class="px-3 py-4 text-right font-semibold text-fg tabular-nums">
								{formatCost(proj.toDate.knownCostMicroUsd)}
							</td>
							<td class="px-3 py-4 text-right">
								<button
									type="button"
									class={figureClass}
									aria-label="{formatCost(
										proj.thisMonth.knownCostMicroUsd
									)} this month. Open records for {proj.name}"
									onclick={(e) => onOpenProjectRecords(proj, e.currentTarget)}
									>{formatCost(proj.thisMonth.knownCostMicroUsd)}</button
								>
							</td>
							<td class="px-3 py-4 text-right text-fg-secondary tabular-nums">
								{formatCost(proj.lastMonth.knownCostMicroUsd)}
							</td>
							<td class="px-3 py-4 text-right text-fg-secondary tabular-nums">
								{formatChangePercent(proj.changePercent)}
							</td>
							<td class="px-3 py-4 text-right text-fg-muted">
								<span aria-hidden="true">—</span>
								<span class="sr-only">Working time {WORKING_TIME_NOT_TRACKED}</span>
							</td>
							<td
								class="py-4 pr-4 pl-3 text-right text-fg-secondary tabular-nums min-[640px]:pr-6 min-[1024px]:pr-8"
							>
								{formatCount(proj.itemsDone)}
							</td>
						</tr>
					{/each}
					{#if showNotOnTask && notOnTask}
						<tr
							class="border-t border-t-line border-b border-line-subtle align-top"
							data-org-project="not-on-a-task"
						>
							<th
								scope="row"
								class="py-4 pr-3 pl-4 text-left font-normal min-[640px]:pl-6 min-[1024px]:pl-8"
							>
								<span class="font-semibold text-fg">Not on a task</span>
								<p class="mt-0.5 text-xs/5 text-fg-muted">
									Team and organization overhead with no project recorded (this month).
								</p>
							</th>
							<td class="px-3 py-4 text-right text-fg-muted">—</td>
							<td class="px-3 py-4 text-right">
								<button
									type="button"
									class={figureClass}
									aria-label="{formatCost(
										notOnTask.knownCostMicroUsd
									)} this month. Open records for Not on a task"
									onclick={(e) => onOpenNotOnTaskRecords(e.currentTarget)}
									>{formatCost(notOnTask.knownCostMicroUsd)}</button
								>
							</td>
							<td class="px-3 py-4 text-right text-fg-muted">—</td>
							<td class="px-3 py-4 text-right text-fg-muted">—</td>
							<td class="px-3 py-4 text-right text-fg-muted">—</td>
							<td class="py-4 pr-4 pl-3 text-right text-fg-muted min-[640px]:pr-6 min-[1024px]:pr-8"
								>—</td
							>
						</tr>
					{/if}
				</tbody>
			</table>

			<ul role="list" class="divide-y divide-line-subtle border-y border-line-subtle @4xl:hidden">
				{#each visibleProjects as proj (proj.projectId)}
					{@const rowInc = incompleteText(proj.toDate)}
					<li class="py-4 {gutters}" data-org-project={proj.projectId}>
						<div class="flex flex-wrap items-start justify-between gap-x-4 gap-y-1">
							<p class="min-w-0 flex-1 basis-40 text-sm/6 font-semibold text-fg">
								<a
									href={resolve(`/projects/${encodeURIComponent(proj.projectId)}/insights` as '/')}
									class="rounded-sm text-link hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
									>{proj.name}</a
								>
								<span class="ml-1.5 font-mono text-xs/5 font-normal text-fg-muted">
									P-{proj.projectNumber}
								</span>
								{#if proj.archived}
									<span class="ml-1.5 text-xs/5 font-normal text-fg-muted">archived</span>
								{/if}
							</p>
							<span class="text-sm/6 font-semibold text-fg tabular-nums">
								{formatCost(proj.toDate.knownCostMicroUsd)} to date
							</span>
						</div>
						<div
							class="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs/5 text-fg-secondary tabular-nums"
						>
							<span>
								This month:
								<button
									type="button"
									class={figureClass}
									aria-label="{formatCost(
										proj.thisMonth.knownCostMicroUsd
									)} this month. Open records for {proj.name}"
									onclick={(e) => onOpenProjectRecords(proj, e.currentTarget)}
									>{formatCost(proj.thisMonth.knownCostMicroUsd)}</button
								>
							</span>
							<span>
								· Last month {formatCost(proj.lastMonth.knownCostMicroUsd)} ({formatChangePercent(
									proj.changePercent
								)}) · Working: — ({WORKING_TIME_NOT_TRACKED}) · {formatCount(proj.itemsDone)}
								{proj.itemsDone === 1 ? 'item' : 'items'} done
							</span>
						</div>
						{#if rowInc}
							<p class="mt-1 text-xs/5 text-fg-muted">{rowInc}</p>
						{/if}
					</li>
				{/each}
				{#if showNotOnTask && notOnTask}
					<li class="border-t border-line py-4 {gutters}" data-org-project="not-on-a-task">
						<div class="flex flex-wrap items-start justify-between gap-x-4 gap-y-1">
							<div class="min-w-0 flex-1 basis-40">
								<p class="text-sm/6 font-semibold text-fg">Not on a task</p>
								<p class="mt-0.5 text-xs/5 text-fg-muted">
									Team and organization overhead with no project recorded (this month).
								</p>
							</div>
							<button
								type="button"
								class="text-sm/6 {figureClass}"
								aria-label="{formatCost(
									notOnTask.knownCostMicroUsd
								)} this month. Open records for Not on a task"
								onclick={(e) => onOpenNotOnTaskRecords(e.currentTarget)}
								>{formatCost(notOnTask.knownCostMicroUsd)}</button
							>
						</div>
					</li>
				{/if}
			</ul>
			<p class="mt-3 text-xs/5 text-fg-muted {gutters}">
				Working time: {WORKING_TIME_NOT_TRACKED}.
			</p>
		{/if}
	</div>
</section>
