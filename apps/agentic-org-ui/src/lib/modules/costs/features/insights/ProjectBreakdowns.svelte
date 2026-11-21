<!--
  @component
  Project › Insights breakdowns (costs-and-insights-ux §1, INS-2):
  tabs switch between Team, Consumer, Model, Task type, and Work type. Supports text filtering,
  accessible column sorting (`aria-sort`), share-of-cost bars, working time honestly marked "— (not tracked yet)",
  and drill-down into the effective usage records behind every figure. Reflows to stacked cards on narrow screens.
-->
<script lang="ts">
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ArrowUpDownIcon from '@lucide/svelte/icons/arrow-up-down';
	import { gutters, inputClass } from '$lib/components/classes';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { formatMedianDuration, WORKING_TIME_NOT_TRACKED } from '../../duration';
	import {
		filterAndSortBreakdowns,
		type BreakdownSortKey,
		type SortDir
	} from '../../insights-helpers';
	import { formatCost, formatCount, formatTokens } from '../../money';
	import { incompleteText, shareOf } from '../../present';
	import type {
		InsightBreakdownRow,
		ProjectDimension,
		ProjectInsights,
		UsageTotals
	} from '../../types';

	let {
		breakdowns,
		total,
		onopen
	}: {
		breakdowns: ProjectInsights['breakdowns'];
		total: UsageTotals;
		onopen: (row: InsightBreakdownRow, opener: HTMLElement) => void;
	} = $props();

	const uid = $props.id();
	const headingId = `${uid}-heading`;
	const filterId = `${uid}-filter`;

	const DIMENSIONS: { id: ProjectDimension; label: string; header: string }[] = [
		{ id: 'team', label: 'Team', header: 'Team' },
		{ id: 'consumer', label: 'Consumer', header: 'Consumer' },
		{ id: 'model', label: 'Model', header: 'Model' },
		{ id: 'task_type', label: 'Task type', header: 'Task type' },
		{ id: 'work_type', label: 'Work type', header: 'Work type' }
	];

	let dimension = $state<ProjectDimension>('team');
	let query = $state('');
	let sortKey = $state<BreakdownSortKey>('cost');
	let sortDir = $state<SortDir>('desc');

	const currentDim = $derived(DIMENSIONS.find((d) => d.id === dimension)!);
	const rawRows = $derived.by(() => {
		switch (dimension) {
			case 'team':
				return breakdowns.byTeam;
			case 'consumer':
				return breakdowns.byConsumer;
			case 'model':
				return breakdowns.byModel;
			case 'task_type':
				return breakdowns.byTaskType;
			case 'work_type':
				return breakdowns.byWorkType;
		}
	});
	const rows = $derived(filterAndSortBreakdowns(rawRows, query, sortKey, sortDir));

	function toggleSort(key: BreakdownSortKey) {
		if (sortKey === key) {
			sortDir = sortDir === 'desc' ? 'asc' : 'desc';
		} else {
			sortKey = key;
			sortDir = key === 'label' ? 'asc' : 'desc';
		}
	}

	function ariaSort(key: BreakdownSortKey): 'ascending' | 'descending' | 'none' {
		if (sortKey !== key) return 'none';
		return sortDir === 'asc' ? 'ascending' : 'descending';
	}

	function tabKey(event: KeyboardEvent, index: number) {
		const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
		if (!step && event.key !== 'Home' && event.key !== 'End') return;
		event.preventDefault();
		const next =
			event.key === 'Home'
				? 0
				: event.key === 'End'
					? DIMENSIONS.length - 1
					: (index + step + DIMENSIONS.length) % DIMENSIONS.length;
		dimension = DIMENSIONS[next].id;
		document.getElementById(`${headingId}-tab-${DIMENSIONS[next].id}`)?.focus();
	}

	const openLabel = (row: InsightBreakdownRow) =>
		row.totals.knownCostMicroUsd === null
			? `Cost unknown. Open the usage records for ${row.label}`
			: `${formatCost(row.totals.knownCostMicroUsd)}. Open the usage records for ${row.label}`;

	const figureClass =
		'inline-flex min-h-6 items-center rounded-sm font-semibold text-link tabular-nums hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11';
	const sortBtnClass =
		'inline-flex items-center gap-1 rounded-sm font-semibold text-fg-muted hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus';
</script>

{#snippet sortIcon(key: BreakdownSortKey)}
	{#if sortKey === key && sortDir === 'asc'}
		<ArrowUpIcon class="size-3.5" aria-hidden="true" />
	{:else if sortKey === key && sortDir === 'desc'}
		<ArrowDownIcon class="size-3.5" aria-hidden="true" />
	{:else}
		<ArrowUpDownIcon class="size-3.5 opacity-60" aria-hidden="true" />
	{/if}
{/snippet}

{#snippet shareBar(row: InsightBreakdownRow)}
	{@const pct = shareOf(row.totals.knownCostMicroUsd, total.knownCostMicroUsd)}
	<span class="flex items-center gap-x-2">
		<span aria-hidden="true" class="h-2 w-16 overflow-hidden rounded-full bg-surface-overlay">
			<span class="block h-full rounded-full bg-brand" style:width="{pct ?? 0}%"></span>
		</span>
		<span class="w-9 text-right text-xs/5 text-fg-secondary tabular-nums">
			{pct === undefined ? '—' : `${pct}%`}
		</span>
	</span>
{/snippet}

<section
	aria-labelledby={headingId}
	class="border-t border-line-subtle py-6"
	data-testid="project-breakdowns"
>
	<div class="flex flex-wrap items-end justify-between gap-4 {gutters}">
		<div>
			<h3 id={headingId} class="text-base/7 font-semibold text-fg">Breakdowns</h3>
			<p class="mt-1 text-sm/6 text-fg-muted">
				Select a cost figure to inspect the effective usage records behind it.
			</p>
		</div>
		<div class="w-full min-w-0 min-[640px]:w-64">
			<Label for={filterId} class="sr-only">Filter {currentDim.label.toLowerCase()} rows</Label>
			<Input
				id={filterId}
				type="search"
				bind:value={query}
				placeholder="Filter {currentDim.label.toLowerCase()}…"
				class="w-full {inputClass}"
				data-testid="breakdown-filter"
			/>
		</div>
	</div>

	<div class="mt-4 border-b border-line-subtle {gutters}">
		<div
			role="tablist"
			aria-label="Break project cost down by"
			class="-mb-px flex flex-wrap gap-x-6"
		>
			{#each DIMENSIONS as d, i (d.id)}
				<button
					type="button"
					role="tab"
					id="{headingId}-tab-{d.id}"
					aria-selected={dimension === d.id}
					aria-controls="{headingId}-panel"
					tabindex={dimension === d.id ? 0 : -1}
					onclick={() => (dimension = d.id)}
					onkeydown={(e) => tabKey(e, i)}
					class="inline-flex min-h-11 items-center border-b-2 px-1 text-sm/6 font-semibold focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus min-[640px]:min-h-10 {dimension ===
					d.id
						? 'border-brand text-fg'
						: 'border-transparent text-fg-muted hover:border-line-strong hover:text-fg'}"
					>{d.label}</button
				>
			{/each}
		</div>
	</div>

	<div
		id="{headingId}-panel"
		role="tabpanel"
		aria-labelledby="{headingId}-tab-{dimension}"
		class="@container"
	>
		{#if rows.length === 0}
			<p class="py-8 text-sm/6 text-fg-muted {gutters}" data-testid="breakdown-empty">
				{query.trim()
					? `No ${currentDim.label.toLowerCase()} rows match "${query.trim()}".`
					: 'No breakdown records in this period.'}
			</p>
		{:else}
			<table class="hidden w-full text-sm/6 @4xl:table" data-testid="project-breakdown-table">
				<caption class="sr-only">Project cost by {currentDim.label.toLowerCase()}</caption>
				<thead>
					<tr class="border-b border-line text-left text-xs/5 font-semibold text-fg-muted">
						<th
							scope="col"
							aria-sort={ariaSort('label')}
							class="py-3 pr-3 pl-4 min-[640px]:pl-6 min-[1024px]:pl-8"
						>
							<button type="button" class={sortBtnClass} onclick={() => toggleSort('label')}>
								{currentDim.header}
								{@render sortIcon('label')}
							</button>
						</th>
						<th scope="col" aria-sort={ariaSort('cost')} class="px-3 py-3 text-right">
							<button
								type="button"
								class="{sortBtnClass} ml-auto"
								onclick={() => toggleSort('cost')}
							>
								Cost (USD)
								{@render sortIcon('cost')}
							</button>
						</th>
						<th scope="col" class="px-3 py-3">Share</th>
						<th scope="col" aria-sort={ariaSort('tokens')} class="px-3 py-3 text-right">
							<button
								type="button"
								class="{sortBtnClass} ml-auto"
								onclick={() => toggleSort('tokens')}
							>
								Tokens
								{@render sortIcon('tokens')}
							</button>
						</th>
						<th scope="col" class="px-3 py-3 text-right">Working</th>
						<th scope="col" aria-sort={ariaSort('itemsDone')} class="px-3 py-3 text-right">
							<button
								type="button"
								class="{sortBtnClass} ml-auto"
								onclick={() => toggleSort('itemsDone')}
							>
								Items
								{@render sortIcon('itemsDone')}
							</button>
						</th>
						<th scope="col" aria-sort={ariaSort('costPerTask')} class="px-3 py-3 text-right">
							<button
								type="button"
								class="{sortBtnClass} ml-auto"
								onclick={() => toggleSort('costPerTask')}
							>
								Cost/item
								{@render sortIcon('costPerTask')}
							</button>
						</th>
						<th
							scope="col"
							aria-sort={ariaSort('medianLead')}
							class="py-3 pr-4 pl-3 text-right min-[640px]:pr-6 min-[1024px]:pr-8"
						>
							<button
								type="button"
								class="{sortBtnClass} ml-auto"
								onclick={() => toggleSort('medianLead')}
							>
								Median lead
								{@render sortIcon('medianLead')}
							</button>
						</th>
					</tr>
				</thead>
				<tbody>
					{#each rows as row (row.id)}
						{@const inc = incompleteText(row.totals)}
						{@const lead = formatMedianDuration(row.medianLeadSeconds, row.sampleCount)}
						<tr
							class="border-b border-line-subtle align-top {row.overhead
								? 'border-t border-t-line'
								: ''}"
							data-breakdown-row={row.id}
						>
							<th
								scope="row"
								class="py-4 pr-3 pl-4 text-left font-normal min-[640px]:pl-6 min-[1024px]:pl-8"
							>
								<span class="font-semibold text-fg [overflow-wrap:anywhere]">{row.label}</span>
								{#if row.subtitle}
									<span class="ml-1.5 text-xs/5 text-fg-muted">{row.subtitle}</span>
								{/if}
								{#if row.archived}
									<span class="ml-1.5 text-xs/5 text-fg-muted">archived</span>
								{/if}
								{#if inc}
									<p class="mt-1 text-xs/5 text-fg-muted" data-testid="row-incomplete">{inc}</p>
								{/if}
							</th>
							<td class="px-3 py-4 text-right">
								<button
									type="button"
									class={figureClass}
									aria-label={openLabel(row)}
									onclick={(e) => onopen(row, e.currentTarget)}
									>{formatCost(row.totals.knownCostMicroUsd)}</button
								>
							</td>
							<td class="px-3 py-4">{@render shareBar(row)}</td>
							<td class="px-3 py-4 text-right text-fg-secondary tabular-nums">
								{formatTokens(row.totals.knownTokens)}
							</td>
							<td class="px-3 py-4 text-right text-fg-muted">
								<span aria-hidden="true">—</span>
								<span class="sr-only">Working time {WORKING_TIME_NOT_TRACKED}</span>
							</td>
							<td class="px-3 py-4 text-right text-fg-secondary tabular-nums">
								{row.itemsDone === null ? '—' : formatCount(row.itemsDone)}
							</td>
							<td class="px-3 py-4 text-right text-fg-secondary tabular-nums">
								{formatCost(row.costPerTaskMicroUsd)}
							</td>
							<td
								class="py-4 pr-4 pl-3 text-right text-fg-secondary tabular-nums min-[640px]:pr-6 min-[1024px]:pr-8"
							>
								<span>{lead.value}</span>
								{#if row.itemsDone !== null && row.sampleCount > 0 && row.sampleCount < 5}
									<span class="block text-xs/5 text-fg-muted">{row.sampleCount} items</span>
								{/if}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>

			<ul role="list" class="divide-y divide-line-subtle border-y border-line-subtle @4xl:hidden">
				{#each rows as row (row.id)}
					{@const inc = incompleteText(row.totals)}
					{@const lead = formatMedianDuration(row.medianLeadSeconds, row.sampleCount)}
					<li
						class="py-4 {gutters} {row.overhead ? 'border-t border-line' : ''}"
						data-breakdown-row={row.id}
					>
						<div class="flex flex-wrap items-start justify-between gap-x-4 gap-y-1">
							<p
								class="min-w-0 flex-1 basis-40 text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]"
							>
								{row.label}{#if row.subtitle}
									<span class="ml-1.5 text-xs/5 font-normal text-fg-muted">{row.subtitle}</span>
								{/if}
							</p>
							<button
								type="button"
								class="text-sm/6 {figureClass}"
								aria-label={openLabel(row)}
								onclick={(e) => onopen(row, e.currentTarget)}
								>{formatCost(row.totals.knownCostMicroUsd)}</button
							>
						</div>
						<div class="mt-2">{@render shareBar(row)}</div>
						<p class="mt-2 text-xs/5 text-fg-muted tabular-nums">
							{formatTokens(row.totals.knownTokens)} tokens · Working: — ({WORKING_TIME_NOT_TRACKED}){#if row.itemsDone !== null}
								· {formatCount(row.itemsDone)} items · {formatCost(row.costPerTaskMicroUsd)}/item ·
								Median lead {lead.value}
							{/if}
						</p>
						{#if inc}
							<p class="mt-1 text-xs/5 text-fg-muted" data-testid="row-incomplete">{inc}</p>
						{/if}
					</li>
				{/each}
			</ul>
			<p class="mt-3 text-xs/5 text-fg-muted {gutters}">
				Working time: {WORKING_TIME_NOT_TRACKED}. Medians require at least 5 completed items.
			</p>
		{/if}
	</div>
</section>
