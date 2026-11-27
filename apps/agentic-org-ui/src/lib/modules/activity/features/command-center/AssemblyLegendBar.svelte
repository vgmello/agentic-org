<script lang="ts">
	import type { ActivityCommandCenterModel } from './assembly-model';

	let {
		model,
		projectScope = false,
		dataUnavailable = false
	}: {
		model: ActivityCommandCenterModel;
		projectScope?: boolean;
		dataUnavailable?: boolean;
	} = $props();
</script>

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

<style>
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
	.legend-item[data-stage='agents'] {
		--stage-color: var(--tone-accent-ink);
	}
	.legend-item[data-stage='ci'] {
		--stage-color: var(--exec-sky);
	}
	.legend-item[data-stage='review'] {
		--stage-color: var(--exec-warn);
	}
	.legend-item[data-stage='merged'] {
		--stage-color: var(--exec-ok);
	}
	.legend-dot {
		width: 8px;
		height: 8px;
		border-radius: 2px;
		background: var(--stage-color);
		flex-shrink: 0;
	}
	.assembly-overall-progress {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		color: var(--fg-secondary);
		font-size: 0.75rem;
		font-weight: 500;
		white-space: nowrap;
	}
	.assembly-overall-progress progress {
		appearance: none;
		width: 78px;
		height: 7px;
		border: 1px solid var(--line-strong);
		border-radius: var(--exec-bar-radius, 999px);
		background: var(--exec-surface);
		color: var(--tone-accent-ink);
		overflow: hidden;
	}
	.assembly-overall-progress progress::-webkit-progress-bar {
		border-radius: inherit;
		background: var(--exec-surface);
	}
	.assembly-overall-progress progress::-webkit-progress-value,
	.assembly-overall-progress progress::-moz-progress-bar {
		border-radius: inherit;
		background: var(--tone-accent-ink);
	}
	.assembly-overall-progress progress.is-complete::-webkit-progress-value,
	.assembly-overall-progress progress.is-complete::-moz-progress-bar {
		background: var(--exec-ok);
	}
	@media (width < 40rem) {
		.assembly-legend {
			padding: 12px;
		}
	}
</style>
