<!--
  @component
  Current project delivery section (Work by stage & Track readiness) for ProjectInsightsView.
-->
<script lang="ts">
	import ArrowUpRight from '@lucide/svelte/icons/arrow-up-right';
	import { resolve } from '$app/paths';
	import { gutters } from '$lib/components/classes';
	import type { ProjectDeliverySummary } from '../../delivery-summary';

	let {
		projectId,
		projectName,
		deliveryModel,
		displayMode
	}: {
		projectId: string;
		projectName: string;
		deliveryModel: ProjectDeliverySummary;
		displayMode: 'chart' | 'table';
	} = $props();

	const stageSummaryRows = $derived.by(() => {
		const counts = deliveryModel.stageCounts;
		const total = Math.max(1, deliveryModel.totalItems);
		return [
			{
				id: 'queue',
				label: 'Queue',
				count: counts.queue,
				pct: Math.round((counts.queue * 100) / total)
			},
			{
				id: 'agents',
				label: 'Agents',
				count: counts.agents,
				pct: Math.round((counts.agents * 100) / total)
			},
			{
				id: 'ci',
				label: 'Ready for checks',
				count: counts.ci,
				pct: Math.round((counts.ci * 100) / total)
			},
			{
				id: 'review',
				label: 'Human review',
				count: counts.review,
				pct: Math.round((counts.review * 100) / total)
			},
			{
				id: 'merged',
				label: 'Merged',
				count: counts.merged,
				pct: Math.round((counts.merged * 100) / total)
			}
		];
	});

	const readinessTracks = $derived(deliveryModel.tracks);
</script>

<section
	aria-labelledby="release-delivery-heading"
	class="border-t border-line-subtle py-6 {gutters}"
	data-testid="insights-release-delivery"
>
	<div class="flex flex-wrap items-baseline justify-between gap-2">
		<div>
			<h3 id="release-delivery-heading" class="text-base/7 font-semibold text-fg">
				Current project delivery
			</h3>
			<p class="mt-1 text-sm/6 text-fg-muted">
				Live pipeline stages and track readiness across {projectName}.
			</p>
		</div>
		<a
			href={resolve(`/projects/${encodeURIComponent(projectId)}/activity` as '/')}
			class="inline-flex min-h-9 items-center gap-1 rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
		>
			<span>Open Assembly line</span>
			<ArrowUpRight class="size-4" aria-hidden="true" />
		</a>
	</div>

	<div class="mt-4 grid grid-cols-1 gap-4 lg:grid-cols-2">
		<div class="insights-stage-card rounded-lg border border-line-subtle p-4">
			<h4 class="text-sm/6 font-semibold text-fg">Work by stage</h4>
			{#if deliveryModel.totalItems === 0}
				<p class="mt-2 text-xs/5 text-fg-muted">No active delivery tasks in this project yet.</p>
			{:else if displayMode === 'chart'}
				<ul role="list" class="mt-3 grid gap-2.5">
					{#each stageSummaryRows as row (row.id)}
						<li class="grid grid-cols-[7.5rem_minmax(0,1fr)_3.5rem] items-center gap-2 text-xs/5">
							<span class="text-fg-secondary">{row.label}</span>
							<div class="h-2 overflow-hidden rounded-full bg-bar-track">
								<span
									class="block h-full rounded-full"
									style:width="{Math.max(row.count > 0 ? 4 : 0, row.pct)}%"
									style:background={row.id === 'merged'
										? 'var(--tone-ok-bar)'
										: row.id === 'review'
											? 'var(--tone-warn-bar)'
											: 'var(--tone-accent-bar)'}
								></span>
							</div>
							<span class="text-right font-semibold text-fg tabular-nums">{row.count}</span>
						</li>
					{/each}
				</ul>
			{:else}
				<div class="mt-3 overflow-x-auto">
					<table class="w-full text-left text-xs/5" data-testid="insights-stage-table">
						<thead>
							<tr class="border-b border-line text-fg-muted">
								<th scope="col" class="py-1.5 pr-3">Stage</th>
								<th scope="col" class="py-1.5 text-right">Items</th>
								<th scope="col" class="py-1.5 text-right">Share</th>
							</tr>
						</thead>
						<tbody class="divide-y divide-line-subtle">
							{#each stageSummaryRows as row (row.id)}
								<tr>
									<th scope="row" class="py-1.5 pr-3 font-medium text-fg">{row.label}</th>
									<td class="py-1.5 text-right text-fg-secondary tabular-nums">{row.count}</td>
									<td class="py-1.5 text-right text-fg-secondary tabular-nums">{row.pct}%</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</div>

		<div class="rounded-lg border border-line-subtle bg-surface-raised/30 p-4">
			<h4 class="text-sm/6 font-semibold text-fg">Track readiness</h4>
			{#if readinessTracks.length === 0}
				<p class="mt-2 text-xs/5 text-fg-muted">No project tracks recorded yet.</p>
			{:else if displayMode === 'chart'}
				<ul role="list" class="mt-3 grid gap-2.5">
					{#each readinessTracks as track (track.id)}
						<li class="grid grid-cols-[minmax(0,1fr)_6.5rem] items-center gap-2 text-xs/5">
							<div class="min-w-0">
								<span class="block truncate font-medium text-fg">{track.name}</span>
								<div class="mt-1 h-1.5 overflow-hidden rounded-full bg-bar-track">
									<span
										class="block h-full rounded-full"
										style:width="{track.pct}%"
										style:background={track.isComplete
											? 'var(--tone-ok-bar)'
											: 'var(--tone-accent-bar)'}
									></span>
								</div>
							</div>
							<span class="text-right font-semibold text-fg-secondary tabular-nums">
								{track.doneCount}/{track.totalCount} ({track.pct}%)
							</span>
						</li>
					{/each}
				</ul>
			{:else}
				<div class="mt-3 overflow-x-auto">
					<table class="w-full text-left text-xs/5">
						<thead>
							<tr class="border-b border-line text-fg-muted">
								<th scope="col" class="py-1.5 pr-3">Track</th>
								<th scope="col" class="py-1.5 text-right">Merged</th>
								<th scope="col" class="py-1.5 text-right">Readiness</th>
							</tr>
						</thead>
						<tbody class="divide-y divide-line-subtle">
							{#each readinessTracks as track (track.id)}
								<tr>
									<th scope="row" class="py-1.5 pr-3 font-medium text-fg">{track.name}</th>
									<td class="py-1.5 text-right text-fg-secondary tabular-nums">
										{track.doneCount}/{track.totalCount}
									</td>
									<td class="py-1.5 text-right text-fg-secondary tabular-nums">{track.pct}%</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</div>
	</div>
</section>

<style>
	.insights-stage-card {
		background: var(--workspace-secondary-surface, var(--surface-raised));
	}
</style>
