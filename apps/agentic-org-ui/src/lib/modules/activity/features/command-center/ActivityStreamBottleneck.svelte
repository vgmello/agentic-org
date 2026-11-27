<script lang="ts">
	import type { BottleneckSummary } from './assembly-model';

	let { bottleneck }: { bottleneck: BottleneckSummary } = $props();
</script>

<div class="stream-bottleneck activity-bottleneck">
	<div class="stream-bottleneck-main">
		<span class="stream-bottleneck-label">Open mix</span>
		<div class="stream-stage-bar" role="img" aria-label={bottleneck.legend}>
			{#if bottleneck.agentsPct > 0}
				<span
					class="seg-agents"
					style:width={`${bottleneck.agentsPct}%`}
					title={`${bottleneck.agentsPct}% agents`}
				></span>
			{/if}
			{#if bottleneck.ciPct > 0}
				<span
					class="seg-ci"
					style:width={`${bottleneck.ciPct}%`}
					title={`${bottleneck.ciPct}% CI / ready for checks`}
				></span>
			{/if}
			{#if bottleneck.reviewPct > 0}
				<span
					class="seg-review"
					style:width={`${bottleneck.reviewPct}%`}
					title={`${bottleneck.reviewPct}% waiting on you`}
				></span>
			{/if}
			{#if bottleneck.queuePct > 0}
				<span
					class="seg-queue"
					style:width={`${bottleneck.queuePct}%`}
					title={`${bottleneck.queuePct}% queued`}
				></span>
			{/if}
		</div>
		<span class="stream-bottleneck-legend tabular-nums">{bottleneck.legend}</span>
	</div>
	<span class="stream-bottleneck-note">{bottleneck.note}</span>
</div>

<style>
	.stream-bottleneck {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 8px 16px;
		padding: 8px var(--activity-gutter, 16px);
		border-bottom: 1px solid var(--line);
		background: var(--workspace-secondary-surface, var(--exec-well));
		font-size: 0.75rem;
	}
	.stream-bottleneck-main {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 10px;
		flex: 1 1 260px;
		min-width: 0;
	}
	.stream-bottleneck-label {
		color: var(--exec-body);
		font-weight: 600;
		flex-shrink: 0;
	}
	.stream-stage-bar {
		display: flex;
		gap: 1px;
		min-width: 92px;
		max-width: 180px;
		flex: 1;
		height: 6px;
		overflow: hidden;
		border-radius: var(--exec-bar-radius, 999px);
		background: var(--bar-track);
	}
	.stream-stage-bar .seg-agents {
		background: var(--exec-seg-agents);
	}
	.stream-stage-bar .seg-ci {
		background: var(--exec-seg-ci);
	}
	.stream-stage-bar .seg-review {
		background: var(--exec-warn);
	}
	.stream-stage-bar .seg-queue {
		background: var(--exec-seg-queued);
	}
	.stream-bottleneck-legend,
	.stream-bottleneck-note {
		color: var(--fg-secondary);
	}
</style>
