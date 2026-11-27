<script lang="ts">
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Layers from '@lucide/svelte/icons/layers';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import AssemblyLegendBar from './AssemblyLegendBar.svelte';
	import AssemblyStageCell from './AssemblyStageCell.svelte';
	import AssemblySummaryStrip from './AssemblySummaryStrip.svelte';
	import type { ActivityCommandCenterModel, FeedPhase } from './assembly-model';

	let {
		model,
		loading = false,
		unavailableReason = undefined,
		inboxPhase = 'ready',
		projectScope = false,
		hasActivityEntries = false,
		onRetry = undefined
	}: {
		model: ActivityCommandCenterModel;
		loading?: boolean;
		unavailableReason?: string;
		inboxPhase?: FeedPhase;
		projectScope?: boolean;
		hasActivityEntries?: boolean;
		onRetry?: () => void;
	} = $props();

	let tableEl = $state<HTMLTableElement | null>(null);
	let prevChipState: Record<string, { stage: string; rect: DOMRect }> = {};

	$effect.pre(() => {
		void model.milestones;
		if (!tableEl || typeof window === 'undefined') return;
		if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
		const next: Record<string, { stage: string; rect: DOMRect }> = {};
		for (const el of tableEl.querySelectorAll<HTMLElement>('.assembly-chip[data-chip-id]')) {
			const id = el.dataset.chipId;
			const stage = el.dataset.stage;
			if (id && stage) next[id] = { stage, rect: el.getBoundingClientRect() };
		}
		prevChipState = next;
	});

	$effect(() => {
		void model.milestones;
		if (!tableEl || Object.keys(prevChipState).length === 0 || typeof window === 'undefined')
			return;
		if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
		for (const el of tableEl.querySelectorAll<HTMLElement>('.assembly-chip[data-chip-id]')) {
			const id = el.dataset.chipId;
			const stage = el.dataset.stage;
			if (!id || !stage) continue;
			const prev = prevChipState[id];
			if (!prev || prev.stage === stage) continue;
			const cur = el.getBoundingClientRect();
			const dx = prev.rect.left - cur.left;
			const dy = prev.rect.top - cur.top;
			if (Math.abs(dx) < 1 && Math.abs(dy) < 1) continue;
			el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: 'translate(0, 0)' }], {
				duration: 320,
				easing: 'cubic-bezier(0.2, 0.7, 0.25, 1)'
			});
		}
	});

	const dataUnavailable = $derived(loading || Boolean(unavailableReason));
	const totalTracks = $derived(
		model.milestones.reduce((sum, milestone) => sum + milestone.lanes.length, 0)
	);

	function handleRetryClick(event: MouseEvent) {
		const section = (event.currentTarget as HTMLElement | null)?.closest<HTMLElement>(
			'.assembly-view'
		);
		section?.focus();
		onRetry?.();
	}
</script>

<section class="assembly-view" tabindex="-1" aria-label="Assembly line pipeline">
	<AssemblySummaryStrip {model} {loading} {unavailableReason} {inboxPhase} {projectScope} />

	{#if loading}
		<div class="assembly-empty" role="status" aria-live="polite">
			<Layers class="size-7" style="color: var(--fg-secondary);" aria-hidden="true" />
			<h3>Loading assembly line…</h3>
			<p>Fetching live work items and project tracks.</p>
		</div>
	{:else if unavailableReason}
		<div class="assembly-empty" role="alert">
			<AlertTriangle class="size-7" style="color: var(--exec-warn);" aria-hidden="true" />
			<h3>Work pipeline unavailable</h3>
			<p>{unavailableReason}</p>
			{#if onRetry}
				<button
					type="button"
					onclick={handleRetryClick}
					class="assembly-retry-btn min-h-9 max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
				>
					<RefreshCw class="size-3.5" aria-hidden="true" />
					<span>Retry</span>
				</button>
			{/if}
		</div>
	{:else if model.milestones.length === 0 || (projectScope && model.totalItems === 0 && !hasActivityEntries)}
		<div class="assembly-empty">
			<Layers class="size-7" style="color: var(--fg-secondary);" aria-hidden="true" />
			<h3>
				{#if projectScope && !hasActivityEntries}
					No activity yet
				{:else if projectScope}
					No tasks on the assembly line yet
				{:else}
					No active projects on the line yet
				{/if}
			</h3>
			<p>
				{#if projectScope && !hasActivityEntries}
					What happens in {model.scopeTitle} shows here as it happens: tasks created, assigned and finished,
					bugs reported, and delivery moving through human review.
				{:else}
					Create a project or add tasks to track delivery from queue through human review.
				{/if}
			</p>
		</div>
	{:else}
		<!-- scrollable table container needs keyboard focus for horizontal scrolling -->
		<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
		<div class="assembly-table-wrap" tabindex="0" role="region" aria-label="Assembly line matrix">
			<table class="assembly-table" bind:this={tableEl}>
				<thead>
					<tr>
						<th scope="col" class="col-track">
							<span>Milestone › Track</span>
							<small>
								{#if projectScope}
									{totalTracks} {totalTracks === 1 ? 'track' : 'tracks'}
								{:else}
									{model.milestones.length}
									{model.milestones.length === 1 ? 'project' : 'projects'}
								{/if}
							</small>
						</th>
						<th scope="col" class="col-stage">
							<span>Queue</span>
							<small>{model.stageCounts.queue} waiting</small>
						</th>
						<th scope="col" class="col-stage">
							<span>Agents</span>
							<small>{model.stageCounts.agents} active</small>
						</th>
						<th scope="col" class="col-stage">
							<span>CI</span>
							<small>{model.ciColumnSubLabel}</small>
						</th>
						<th scope="col" class="col-stage">
							<span
								class={model.stageCounts.review > 0 || inboxPhase === 'error'
									? 'assembly-review-text'
									: ''}
							>
								Human Review
							</span>
							<small>
								{#if inboxPhase === 'loading'}
									checking Inbox…
								{:else if inboxPhase === 'error'}
									{model.stageCounts.review} waiting · Inbox offline
								{:else}
									{model.stageCounts.review} waiting
								{/if}
							</small>
						</th>
						<th scope="col" class="col-stage">
							<span>Merged</span>
							<small>{model.stageCounts.merged} of {model.totalItems}</small>
						</th>
						<th scope="col" class="col-release">
							<span>Into Release</span>
							<small>done / total</small>
						</th>
					</tr>
				</thead>
				<tbody>
					{#each model.milestones as milestone (milestone.id)}
						<tr class="assembly-milestone">
							<th colspan="7" scope="colgroup">
								<div class="assembly-milestone-row">
									<span class="assembly-milestone-name">
										{milestone.name}{#if milestone.key}
											<span class="assembly-milestone-key"> · {milestone.key}</span>
										{/if}
									</span>
									<div class="assembly-progress assembly-milestone-progress">
										<progress
											class={milestone.isComplete ? 'is-complete' : ''}
											value={milestone.doneCount}
											max={Math.max(1, milestone.totalCount)}
											aria-label={`${milestone.name} progress: ${milestone.pct}%`}
										></progress>
										<span class="assembly-milestone-summary tabular-nums">{milestone.summary}</span>
									</div>
								</div>
							</th>
						</tr>

						{#each milestone.lanes as lane (lane.id)}
							<tr class="assembly-lane-row">
								<th scope="row" class="assembly-lane-head">
									<div class="assembly-lane-name">{lane.name}</div>
									<small>{lane.subtitle}</small>
								</th>
								<AssemblyStageCell stage="queue" chips={lane.stages.queue} />
								<AssemblyStageCell stage="agents" chips={lane.stages.agents} />
								<AssemblyStageCell stage="ci" chips={lane.stages.ci} />
								<AssemblyStageCell stage="review" chips={lane.stages.review} />
								<AssemblyStageCell stage="merged" chips={lane.stages.merged} />
								<td>
									<div class="assembly-progress">
										<span class={['assembly-pct tabular-nums', lane.isComplete && 'is-complete']}>
											{lane.pct}%
										</span>
										<progress
											class={lane.isComplete ? 'is-complete' : ''}
											value={lane.doneCount}
											max={Math.max(1, lane.totalCount)}
											aria-label={`${lane.name} release progress: ${lane.pct}%`}
										></progress>
										<span class="tabular-nums">{lane.doneCount}/{lane.totalCount}</span>
									</div>
								</td>
							</tr>
						{/each}
					{/each}
				</tbody>
			</table>
		</div>
	{/if}

	<AssemblyLegendBar {model} {projectScope} {dataUnavailable} />
</section>

<style>
	.assembly-view {
		display: flex;
		flex: 1 0 auto;
		flex-direction: column;
		min-width: 0;
		max-width: 100%;
		overflow: hidden;
		border: 0;
		border-radius: 0;
		background-color: var(--assembly-surface, var(--exec-surface));
	}
	.assembly-review-text {
		color: var(--exec-warn);
	}
	.assembly-empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 8px;
		padding: 48px 24px;
		text-align: center;
	}
	.assembly-empty h3 {
		color: var(--exec-ink);
		font-size: 0.9375rem;
		font-weight: 600;
	}
	.assembly-empty p {
		max-width: 28rem;
		color: var(--fg-secondary);
		font-size: 0.8125rem;
	}
	.assembly-retry-btn {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		margin-top: 4px;
		padding: 0 14px;
		border: 0;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		color: var(--exec-ink);
		font-size: 0.8125rem;
		font-weight: 600;
		cursor: pointer;
	}
	.assembly-retry-btn:hover {
		background: var(--exec-surface-hi);
	}
	.assembly-retry-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.assembly-table-wrap {
		flex: 1 0 auto;
		overflow-x: auto;
		max-width: 100%;
	}
	.assembly-table-wrap:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	.assembly-table {
		width: 100%;
		min-width: 760px;
		border-collapse: collapse;
		table-layout: fixed;
		text-align: start;
		font-size: 0.8125rem;
		line-height: 1.45;
	}
	.assembly-table :is(th, td) {
		padding: 10px 10px;
		text-align: start;
		vertical-align: top;
	}
	:global(html[data-assembly-dividers])
		.assembly-table
		tbody
		tr:not(.assembly-milestone)
		+ tr
		> :global(:is(th, td)) {
		border-block-start: 1px solid var(--line);
	}
	.assembly-table thead th + th,
	.assembly-table tbody td {
		border-inline-start: 1px solid var(--line);
	}
	.assembly-table thead th {
		background: var(--workspace-secondary-surface, var(--exec-well));
		border-block-end: 1px solid var(--line);
		color: var(--exec-ink);
		font-size: 0.75rem;
		font-weight: 600;
	}
	.assembly-table thead th:first-child {
		width: 22%;
	}
	.assembly-table thead th:last-child {
		width: 13%;
	}
	.assembly-table thead th.col-track {
		min-width: 156px;
		padding-inline-start: var(--activity-gutter, 24px);
	}
	.assembly-table thead th.col-release {
		min-width: 112px;
		padding-inline-end: var(--activity-gutter, 24px);
	}
	.assembly-table small {
		display: block;
		margin-top: 2px;
		color: var(--fg-secondary);
		font-size: 0.75rem;
		font-weight: 400;
	}
	.assembly-milestone th {
		padding: 9px 16px;
		border-block: 1px solid var(--line-strong);
		background: var(--tone-accent-bg);
		color: var(--exec-ink);
		font-weight: 650;
	}
	.assembly-milestone-row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px 14px;
		min-width: 0;
	}
	.assembly-milestone-name {
		color: var(--tone-accent-ink);
		font-size: 0.8125rem;
		font-weight: 650;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.assembly-milestone-key,
	.assembly-milestone-summary {
		color: var(--fg-secondary);
		font-weight: 500;
		overflow-wrap: anywhere;
	}
	.assembly-lane-row:hover {
		background: var(--surface-raised);
	}
	.assembly-lane-head {
		padding-inline-start: var(--activity-gutter, 24px);
		font-weight: 550;
	}
	.assembly-lane-name {
		color: var(--exec-ink);
		font-weight: 600;
	}
	.assembly-progress {
		display: grid;
		justify-items: start;
		gap: 3px;
		color: var(--fg-secondary);
		font-size: 0.75rem;
		white-space: nowrap;
	}
	.assembly-milestone-progress {
		display: inline-flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px 12px;
		font-weight: 500;
		white-space: normal;
	}
	.assembly-pct {
		color: var(--exec-ink);
		font-weight: 650;
	}
	.assembly-pct.is-complete {
		color: var(--exec-ok);
	}
	.assembly-progress progress {
		appearance: none;
		width: 78px;
		height: 7px;
		border: 1px solid var(--line-strong);
		border-radius: var(--exec-bar-radius, 999px);
		background: var(--exec-well);
		color: var(--tone-accent-ink);
		overflow: hidden;
	}
	.assembly-milestone .assembly-progress progress {
		width: clamp(96px, 12cqi, 144px);
	}
	.assembly-progress progress::-webkit-progress-bar {
		border-radius: inherit;
		background: var(--exec-well);
	}
	.assembly-progress progress::-webkit-progress-value,
	.assembly-progress progress::-moz-progress-bar {
		border-radius: inherit;
		background: var(--tone-accent-ink);
	}
	.assembly-progress progress.is-complete::-webkit-progress-value,
	.assembly-progress progress.is-complete::-moz-progress-bar {
		background: var(--exec-ok);
	}
	@media (forced-colors: active) {
		.assembly-retry-btn {
			border: 1px solid ButtonText;
		}
	}
</style>
