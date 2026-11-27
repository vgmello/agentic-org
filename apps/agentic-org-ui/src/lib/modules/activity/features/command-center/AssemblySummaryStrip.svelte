<script lang="ts">
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
		projectScope = false
	}: {
		model: ActivityCommandCenterModel;
		loading?: boolean;
		unavailableReason?: string;
		inboxPhase?: FeedPhase;
		projectScope?: boolean;
	} = $props();

	const agentsBusyLabel = $derived(
		model.activeAgentsCount > 0
			? `${model.busyAgentsCount} / ${model.activeAgentsCount}`
			: String(model.stageCounts.agents)
	);
	const dataUnavailable = $derived(loading || Boolean(unavailableReason));
	const summaryTitle = $derived(projectScope ? 'Assembly line' : model.scopeTitle);
	const avgCycleLabel = $derived(formatTelemetryHours(model.avgCycleHours));
</script>

<div class="assembly-summary">
	<div class="min-w-0">
		<div class="assembly-eyebrow">{model.scopeEyebrow}</div>
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
					: `${projectScope ? 'Project' : 'Portfolio'} readiness: ${model.readinessPct}%`}
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

<style>
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
	.assembly-metrics dd.assembly-review-text {
		color: var(--exec-warn);
	}
	.assembly-progress-bar {
		display: block;
		width: clamp(60px, 6cqi, 92px);
		margin-block-start: 4px;
		appearance: none;
		height: 7px;
		border: 1px solid var(--line-strong);
		border-radius: var(--exec-bar-radius, 999px);
		background: var(--exec-well);
		color: var(--tone-accent-ink);
		overflow: hidden;
	}
	.assembly-progress-bar::-webkit-progress-bar {
		border-radius: inherit;
		background: var(--exec-well);
	}
	.assembly-progress-bar::-webkit-progress-value,
	.assembly-progress-bar::-moz-progress-bar {
		border-radius: inherit;
		background: var(--tone-accent-ink);
	}
	.assembly-progress-bar.is-complete::-webkit-progress-value,
	.assembly-progress-bar.is-complete::-moz-progress-bar {
		background: var(--exec-ok);
	}
	@media (width < 40rem) {
		.assembly-summary {
			padding: 12px;
		}
	}
	@media (forced-colors: active) {
		.assembly-live-pill {
			border: 1px solid Highlight;
		}
	}
</style>
