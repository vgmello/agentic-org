<script lang="ts">
	import { resolve } from '$app/paths';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Layers from '@lucide/svelte/icons/layers';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import {
		formatTelemetryHours,
		type ActivityCommandCenterModel,
		type FeedPhase
	} from './assembly-model';

	let {
		model,
		loading = false,
		unavailableReason = undefined,
		inboxPhase = 'ready',
		projectScope = false,
		releaseLabel = undefined,
		hasActivityEntries = false,
		onRetry = undefined
	}: {
		model: ActivityCommandCenterModel;
		loading?: boolean;
		unavailableReason?: string;
		inboxPhase?: FeedPhase;
		projectScope?: boolean;
		releaseLabel?: string;
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

	const agentsBusyLabel = $derived(
		model.activeAgentsCount > 0
			? `${model.busyAgentsCount} / ${model.activeAgentsCount}`
			: String(model.stageCounts.agents)
	);
	const dataUnavailable = $derived(loading || Boolean(unavailableReason));
	const totalTracks = $derived(
		model.milestones.reduce((sum, milestone) => sum + milestone.lanes.length, 0)
	);
	const summaryEyebrow = $derived(
		projectScope ? (releaseLabel ?? model.scopeEyebrow) : model.scopeEyebrow
	);
	const summaryTitle = $derived(projectScope ? 'Assembly line' : model.scopeTitle);
	const avgCycleLabel = $derived(formatTelemetryHours(model.avgCycleHours));

	function handleRetryClick(event: MouseEvent) {
		const section = (event.currentTarget as HTMLElement | null)?.closest<HTMLElement>(
			'.assembly-view'
		);
		section?.focus();
		onRetry?.();
	}
</script>

<section class="assembly-view" tabindex="-1" aria-label="Assembly line pipeline">
	<!-- Summary Strip -->
	<div class="assembly-summary">
		<div class="min-w-0">
			<div class="assembly-eyebrow">{summaryEyebrow}</div>
			<div class="mt-0.5 flex flex-wrap items-center gap-2.5">
				<h2 class="assembly-scope-title">{summaryTitle}</h2>
				<span class="assembly-live-pill">
					<span class="assembly-live-dot" aria-hidden="true"></span>
					<span>
						{#if loading}
							Loading pipeline…
						{:else if unavailableReason}
							Work data unavailable
						{:else}
							{model.liveStatusText}
						{/if}
					</span>
				</span>
			</div>
		</div>

		<dl class="assembly-metrics">
			<div>
				<dt>Readiness</dt>
				<dd class={dataUnavailable ? '' : 'assembly-metric-sky'}>
					{dataUnavailable ? '—' : `${model.readinessPct}%`}
				</dd>
				<progress
					class={['assembly-progress-bar', !dataUnavailable && model.isComplete && 'is-complete']}
					value={dataUnavailable ? 0 : model.mergedItems}
					max={Math.max(1, model.totalItems)}
					aria-label={dataUnavailable
						? 'Readiness unavailable'
						: `${projectScope ? 'Release' : 'Portfolio'} readiness: ${model.readinessPct}%`}
				></progress>
			</div>
			<div>
				<dt>Merged</dt>
				<dd>{dataUnavailable ? '—' : `${model.mergedItems} / ${model.totalItems}`}</dd>
			</div>
			<div>
				<dt>Agents busy</dt>
				<dd>{dataUnavailable ? '—' : agentsBusyLabel}</dd>
			</div>
			<div>
				<dt>Human review</dt>
				<dd
					class={!dataUnavailable && (model.stageCounts.review > 0 || inboxPhase === 'error')
						? 'assembly-review-text'
						: ''}
				>
					{#if dataUnavailable}
						—
					{:else if inboxPhase === 'loading'}
						Checking…
					{:else if inboxPhase === 'error'}
						{model.stageCounts.review} (Inbox unavailable)
					{:else}
						{model.stageCounts.review}
					{/if}
				</dd>
			</div>
			{#if projectScope}
				<div>
					<dt>Avg. cycle time</dt>
					<dd data-testid="assembly-avg-cycle">{dataUnavailable ? '—' : avgCycleLabel}</dd>
				</div>
			{:else}
				<div>
					<dt>Open</dt>
					<dd>{dataUnavailable ? '—' : model.openItems}</dd>
				</div>
			{/if}
		</dl>
	</div>

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
		<!-- 7-Column Swimlane Pipeline Table -->
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

								<!-- Queue -->
								<td>
									<div class="assembly-cell">
										{#if lane.stages.queue.length === 0}
											<span class="assembly-dash" aria-label="None">—</span>
										{:else}
											{#each lane.stages.queue as chip (chip.id)}
												<a
													href={resolve(chip.href as '/work')}
													class="assembly-chip"
													data-chip-id={chip.id}
													data-stage="queue"
													title={chip.tooltip}
												>
													{chip.shortId}
												</a>
											{/each}
										{/if}
									</div>
								</td>

								<!-- Agents -->
								<td>
									<div class="assembly-cell">
										{#if lane.stages.agents.length === 0}
											<span class="assembly-dash" aria-label="None">—</span>
										{:else}
											{#each lane.stages.agents as chip (chip.id)}
												<a
													href={resolve(chip.href as '/work')}
													class="assembly-chip"
													data-chip-id={chip.id}
													data-stage="agents"
													title={chip.tooltip}
												>
													<span class="agent-pulse-dot" aria-hidden="true"></span>
													<span>
														{chip.shortId}{#if chip.agentFirstName}
															· {chip.agentFirstName}
														{/if}
													</span>
												</a>
											{/each}
										{/if}
									</div>
								</td>

								<!-- CI / Checks ready -->
								<td>
									<div class="assembly-cell">
										{#if lane.stages.ci.length === 0}
											<span class="assembly-dash" aria-label="None">—</span>
										{:else}
											{#each lane.stages.ci as chip (chip.id)}
												<a
													href={resolve(chip.href as '/work')}
													class="assembly-chip"
													data-chip-id={chip.id}
													data-stage="ci"
													title={chip.tooltip}
												>
													{chip.shortId}
												</a>
											{/each}
										{/if}
									</div>
								</td>

								<!-- Human Review -->
								<td>
									<div class="assembly-cell">
										{#if lane.stages.review.length === 0}
											<span class="assembly-dash" aria-label="None">—</span>
										{:else}
											{#each lane.stages.review as chip (chip.id)}
												<a
													href={resolve(chip.href as '/inbox')}
													class="assembly-chip"
													data-chip-id={chip.id}
													data-stage="review"
													title={chip.tooltip}
												>
													{chip.shortId}
												</a>
											{/each}
										{/if}
									</div>
								</td>

								<!-- Merged -->
								<td>
									<div class="assembly-cell">
										{#if lane.stages.merged.length === 0}
											<span class="assembly-dash" aria-label="None">—</span>
										{:else}
											{#each lane.stages.merged as chip (chip.id)}
												<a
													href={resolve(chip.href as '/work')}
													class="assembly-chip"
													data-chip-id={chip.id}
													data-stage="merged"
													title={chip.tooltip}
												>
													{chip.shortId}
												</a>
											{/each}
										{/if}
									</div>
								</td>

								<!-- Into Release -->
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

	<!-- Bottom Legend & Overall Readiness Bar -->
	<div class="assembly-legend">
		<div class="assembly-legend-items" aria-label="Pipeline stage legend">
			<span class="legend-item" data-stage="queue">
				<span class="legend-dot" aria-hidden="true"></span>
				<span>Queue</span>
			</span>
			<span class="legend-item" data-stage="agents">
				<span class="legend-dot" aria-hidden="true"></span>
				<span>Agents</span>
			</span>
			<span class="legend-item" data-stage="ci">
				<span class="legend-dot" aria-hidden="true"></span>
				<span>Ready for checks</span>
			</span>
			<span class="legend-item" data-stage="review">
				<span class="legend-dot" aria-hidden="true"></span>
				<span>Human review</span>
			</span>
			<span class="legend-item" data-stage="merged">
				<span class="legend-dot" aria-hidden="true"></span>
				<span>Merged</span>
			</span>
		</div>

		{#if !projectScope}
			<div class="assembly-progress assembly-overall-progress">
				<span class="tabular-nums">
					{#if dataUnavailable}
						—
					{:else}
						{model.readinessPct}% · {model.mergedItems}/{model.totalItems} merged
					{/if}
				</span>
				<progress
					class={!dataUnavailable && model.isComplete ? 'is-complete' : ''}
					value={dataUnavailable ? 0 : model.mergedItems}
					max={Math.max(1, model.totalItems)}
					aria-label={dataUnavailable
						? 'Overall readiness unavailable'
						: `Overall readiness: ${model.readinessPct}%`}
				></progress>
			</div>
		{/if}
	</div>
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

	.assembly-summary {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 12px 20px;
		padding: 14px var(--activity-gutter, 24px);
		border-bottom: 1px solid var(--line);
	}

	.assembly-eyebrow {
		color: var(--tone-accent-ink);
		font-size: 0.6875rem;
		font-weight: 650;
		letter-spacing: 0.06em;
		text-transform: uppercase;
	}

	.assembly-scope-title {
		color: var(--exec-ink);
		font-size: 1.125rem;
		font-weight: 650;
		line-height: 1.3;
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.assembly-live-pill {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 2px 9px;
		border-radius: var(--exec-pill-radius, 999px);
		background: var(--tone-accent-bg);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
		color: var(--tone-accent-ink);
		font-size: 0.75rem;
		font-weight: 550;
	}

	.assembly-live-dot {
		width: 6px;
		height: 6px;
		border-radius: 999px;
		background: var(--tone-accent-ink);
		flex-shrink: 0;
	}

	.assembly-metrics {
		display: flex;
		flex-wrap: wrap;
		gap: 8px 20px;
		margin: 0;
	}

	.assembly-metrics dt {
		color: var(--fg-secondary);
		font-size: 0.6875rem;
		font-weight: 550;
		letter-spacing: 0.04em;
		text-transform: uppercase;
	}

	.assembly-metrics dd {
		margin: 2px 0 0;
		color: var(--exec-ink);
		font-size: 1rem;
		font-weight: 650;
		font-variant-numeric: tabular-nums;
	}

	.assembly-metrics dd.assembly-metric-sky {
		color: var(--exec-sky);
	}

	.assembly-metrics .assembly-progress-bar {
		display: block;
		width: clamp(60px, 6cqi, 92px);
		margin-block-start: 4px;
	}

	.assembly-review-text,
	.assembly-metrics dd.assembly-review-text {
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
		> :is(th, td) {
		border-block-start: 1px solid var(--line);
	}

	.assembly-table tr > :is(th, td) + :is(th, td) {
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

	.assembly-cell {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
	}

	.assembly-chip {
		--stage-color: var(--fg-secondary);
		--stage-bg: var(--exec-well);
		--stage-ring: var(--exec-border-hi);
		position: relative;
		display: inline-flex;
		align-items: center;
		gap: 5px;
		min-height: 36px;
		max-width: 100%;
		padding: 4px 10px;
		border-radius: var(--exec-sm-radius, 6px);
		background: var(--stage-bg);
		box-shadow: inset 0 0 0 1px var(--stage-ring);
		color: var(--stage-color);
		font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
		font-size: 0.75rem;
		font-weight: 550;
		line-height: 1.25;
		text-decoration: none;
		overflow-wrap: anywhere;
		transition: box-shadow 0.15s ease;
	}

	.assembly-chip:hover,
	.assembly-chip:focus-visible {
		box-shadow: inset 0 0 0 1.5px var(--stage-color);
	}

	.assembly-chip:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}

	.assembly-chip[data-stage='agents'],
	.legend-item[data-stage='agents'] {
		--stage-color: var(--tone-accent-ink);
		--stage-bg: var(--tone-accent-bg);
		--stage-ring: var(--tone-accent-ring);
	}

	.assembly-chip[data-stage='ci'],
	.legend-item[data-stage='ci'] {
		--stage-color: var(--exec-sky);
		--stage-bg: var(--exec-sky-bg);
		--stage-ring: var(--exec-sky-ring);
	}

	.assembly-chip[data-stage='review'],
	.legend-item[data-stage='review'] {
		--stage-color: var(--exec-warn);
		--stage-bg: var(--exec-warn-bg);
		--stage-ring: var(--exec-warn-ring);
	}

	.assembly-chip[data-stage='merged'],
	.legend-item[data-stage='merged'] {
		--stage-color: var(--exec-ok);
		--stage-bg: var(--exec-ok-bg);
		--stage-ring: var(--exec-ok-ring);
	}

	.agent-pulse-dot {
		width: 6px;
		height: 6px;
		border-radius: 999px;
		background: var(--tone-accent-ink);
		flex-shrink: 0;
		animation: assembly-pulse 1.8s ease-in-out infinite;
	}

	@keyframes assembly-pulse {
		0%,
		100% {
			opacity: 1;
		}
		50% {
			opacity: 0.35;
		}
	}

	.assembly-dash {
		color: var(--fg-secondary);
		font-size: 0.8125rem;
		line-height: 36px;
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

	.assembly-overall-progress {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		font-weight: 500;
	}

	.assembly-pct {
		color: var(--exec-ink);
		font-weight: 650;
	}

	.assembly-pct.is-complete {
		color: var(--exec-ok);
	}

	.assembly-progress progress,
	.assembly-progress-bar {
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

	.assembly-legend .assembly-progress progress {
		background: var(--exec-surface);
	}

	.assembly-progress progress::-webkit-progress-bar,
	.assembly-progress-bar::-webkit-progress-bar {
		border-radius: inherit;
		background: var(--exec-well);
	}

	.assembly-legend .assembly-progress progress::-webkit-progress-bar {
		background: var(--exec-surface);
	}

	.assembly-progress progress::-webkit-progress-value,
	.assembly-progress-bar::-webkit-progress-value {
		border-radius: inherit;
		background: var(--tone-accent-ink);
	}

	.assembly-progress progress::-moz-progress-bar,
	.assembly-progress-bar::-moz-progress-bar {
		border-radius: inherit;
		background: var(--tone-accent-ink);
	}

	.assembly-progress progress.is-complete::-webkit-progress-value,
	.assembly-progress-bar.is-complete::-webkit-progress-value {
		background: var(--exec-ok);
	}

	.assembly-progress progress.is-complete::-moz-progress-bar,
	.assembly-progress-bar.is-complete::-moz-progress-bar {
		background: var(--exec-ok);
	}

	.assembly-legend {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 8px 16px;
		padding: 11px var(--activity-gutter, 24px);
		background: var(--exec-well);
	}

	.assembly-legend-items {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 14px;
	}

	.legend-item {
		--stage-color: var(--exec-muted);
		display: inline-flex;
		align-items: center;
		gap: 6px;
		color: var(--fg-secondary);
		font-size: 0.75rem;
	}

	.legend-dot {
		width: 8px;
		height: 8px;
		border-radius: 2px;
		background: var(--stage-color);
		flex-shrink: 0;
	}

	@media (width < 40rem), (pointer: coarse) {
		.assembly-chip {
			min-height: 44px;
			padding: 6px 12px;
		}
		.assembly-dash {
			line-height: 44px;
		}
	}

	@media (width < 40rem) {
		.assembly-summary,
		.assembly-legend {
			padding: 12px;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.agent-pulse-dot {
			animation: none;
		}
		.assembly-chip {
			transition: none;
		}
	}

	@media (forced-colors: active) {
		.assembly-chip {
			border: 1px solid CanvasText;
		}
		.assembly-live-pill {
			border: 1px solid Highlight;
		}
		.assembly-retry-btn {
			border: 1px solid ButtonText;
		}
	}
</style>
