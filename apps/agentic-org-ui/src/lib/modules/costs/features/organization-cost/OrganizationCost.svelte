<!--
  @component
  Organization settings › Costs › Organization cost (costs-and-insights-ux §1 Organization › Cost, §3; owner only):
  the period's cost, tokens and records with the change from the previous period of the same length, then the
  breakdown by project (closing with Not on a task), by consumer or by work type. Every figure says what is known:
  estimates read ≈, an unknown sum reads "—" with why, and "Usage incomplete" repeats on every total it affects. Each
  figure opens its records. A viewer who may not see amounts is told so, with no figures at all.
-->
<script lang="ts">
	import CostsError from '../states/CostsError.svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import StatTile from '$lib/components/StatTile.svelte';
	import type { Resource } from '$lib/components/resource.svelte';
	import { formatDayTime } from '../../dates';
	import { formatCount, formatTokens } from '../../money';
	import {
		API_EQUIVALENT_NOTE,
		cacheShareText,
		changeText,
		costValue,
		incompleteText,
		periodLabel,
		periodRange,
		unknownCostReason
	} from '../../present';
	import type { Scoped } from '../../scoped';
	import type {
		BreakdownRow,
		Dimension,
		OrganizationCost,
		PeriodRequest,
		RecordsTarget
	} from '../../types';
	import CostBreakdown from './CostBreakdown.svelte';
	import PeriodMenu from './PeriodMenu.svelte';

	let {
		resource,
		period,
		onperiodchange,
		onopenrecords
	}: {
		resource: Resource<Scoped<OrganizationCost>>;
		period: PeriodRequest;
		onperiodchange: (next: PeriodRequest) => void;
		onopenrecords: (target: RecordsTarget, opener: HTMLElement) => void;
	} = $props();

	const headingId = $props.id();
	const DIMENSIONS: { id: Dimension; label: string; header: string }[] = [
		{ id: 'project', label: 'Project', header: 'Project' },
		{ id: 'consumer', label: 'Consumer', header: 'Consumer' },
		{ id: 'work_type', label: 'Work type', header: 'Work type' }
	];
	let dimension = $state<Dimension>('project');

	const scopedCost = $derived(resource.data);
	const cost = $derived(scopedCost?.allowed ? scopedCost.value : undefined);
	const rows = $derived<BreakdownRow[]>(
		!cost
			? []
			: dimension === 'project'
				? cost.byProject
				: dimension === 'consumer'
					? cost.byConsumer
					: cost.byWorkType
	);
	const current = $derived(DIMENSIONS.find((d) => d.id === dimension)!);

	/** Arrow keys move between the breakdown tabs (in-place tabs: one tab stop). */
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

	const gutters = 'px-4 min-[640px]:px-6 min-[1024px]:px-8';
</script>

<section aria-labelledby={headingId} class="py-6" data-testid="organization-cost">
	<div class="flex flex-wrap items-start justify-between gap-x-6 gap-y-3 {gutters}">
		<div class="min-w-0 flex-1 basis-64">
			<h3 id={headingId} class="text-base/7 font-semibold text-fg">Organization cost</h3>
			<p class="mt-1 max-w-3xl text-sm/6 text-fg-muted">
				What agent work cost, by project, consumer and work type. Amounts are estimates at API list
				prices in US dollars, not invoices.
			</p>
		</div>
		{#if !scopedCost || scopedCost.allowed}
			<PeriodMenu {period} busy={resource.reloading} onchange={onperiodchange} />
		{/if}
	</div>

	<div class="mt-4" aria-busy={resource.reloading}>
		{#if resource.state === 'loading'}
			<div
				role="status"
				aria-busy="true"
				class="grid grid-cols-1 gap-px bg-surface-raised min-[640px]:grid-cols-3"
			>
				<span class="sr-only">Loading the organization's cost</span>
				{#each ['cost', 'tokens', 'records'] as tile (tile)}
					<div aria-hidden="true" class="bg-surface px-4 py-6 min-[640px]:px-6">
						<span
							class="block h-4 w-20 animate-pulse rounded bg-surface-raised motion-reduce:animate-none"
						></span>
						<span
							class="mt-3 block h-8 w-32 animate-pulse rounded bg-surface-overlay motion-reduce:animate-none"
						></span>
					</div>
				{/each}
			</div>
			<div class="mt-6"><SkeletonRows label="the cost breakdown" rows={4} /></div>
		{:else if resource.state === 'unavailable'}
			<p class="text-sm/6 text-fg-muted {gutters}">
				This server doesn't serve the organization's cost yet. Nothing is shown rather than a guess.
			</p>
		{:else if resource.state === 'error'}
			<CostsError
				title="Couldn't load costs"
				message={resource.message}
				onretry={() => resource.reload()}
				retrying={resource.reloading}
			/>
		{:else if scopedCost && !scopedCost.allowed}
			<p class="max-w-3xl text-sm/6 text-fg-secondary {gutters}" data-testid="cost-not-yours">
				Only the organization's owner sees its cost. Project managers see their projects' cost on
				each project's Insights; nobody else sees amounts.
			</p>
		{:else if cost}
			{@const totals = cost.totals}
			{@const incomplete = incompleteText(totals, cost.coverage)}
			<p class="text-xs/5 text-fg-muted {gutters}" data-testid="period-range">
				{periodLabel(cost.period.kind)} · {periodRange(cost.period)} · {cost.period.timeZone}
			</p>
			{#if totals.records === 0}
				<EmptyState
					level={4}
					title="No spend in this period"
					description="No usage records were counted for these dates. Usage that hasn't reported yet isn't here, so this is not proof that nothing ran."
					class="py-12"
				/>
			{:else}
				<dl
					class="mt-3 grid grid-cols-1 gap-px border-y border-line-subtle bg-surface-raised min-[640px]:grid-cols-3"
					data-testid="cost-totals"
				>
					<StatTile
						label="Cost"
						value={costValue(totals)}
						detail={totals.knownCostMicroUsd === null
							? unknownCostReason(totals)
							: changeText(totals.knownCostMicroUsd, cost.previous)}
					/>
					<StatTile
						label="Tokens"
						value={formatTokens(totals.knownTokens)}
						detail={cacheShareText(totals)}
					/>
					<StatTile label="Usage records" value={formatCount(totals.records)} />
				</dl>
				<div class="mt-3 flex flex-col gap-1 text-xs/5 {gutters}">
					{#if totals.includesApiEquivalent}<p class="text-fg-secondary">
							{API_EQUIVALENT_NOTE}
						</p>{/if}
					{#if incomplete}<p class="text-fg-secondary" data-testid="totals-incomplete">
							{incomplete}
						</p>{/if}
					{#each cost.coverage.gaps as gap (gap.source)}
						<p class="text-fg-muted">{gap.source}: {gap.reason}</p>
					{/each}
					<p class="text-fg-muted">
						Counted up to the ledger checkpoint of {formatDayTime(
							new Date(cost.coverage.checkpointAt),
							cost.period.timeZone
						)}.
						<button
							type="button"
							class="inline-flex min-h-6 items-center rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
							onclick={(e) =>
								onopenrecords({ key: { all: true }, label: 'all usage' }, e.currentTarget)}
							>Open all {formatCount(totals.records)} records</button
						>
					</p>
				</div>

				<div class="mt-6 border-b border-line-subtle {gutters}">
					<div
						role="tablist"
						aria-label="Break the cost down by"
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
				<div id="{headingId}-panel" role="tabpanel" aria-labelledby="{headingId}-tab-{dimension}">
					<CostBreakdown
						{rows}
						total={totals}
						nameHeader={current.header}
						caption="Cost by {current.label.toLowerCase()}, {periodLabel(cost.period.kind)}"
						onopen={(row, opener) => onopenrecords({ key: row.key, label: row.label }, opener)}
					/>
				</div>
			{/if}
		{/if}
	</div>
</section>
