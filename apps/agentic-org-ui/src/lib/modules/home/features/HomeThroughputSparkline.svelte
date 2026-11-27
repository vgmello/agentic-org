<script lang="ts">
	let {
		points,
		label = 'Closed per day, last 7 days'
	}: {
		points: readonly number[];
		label?: string;
	} = $props();

	const hasActivity = $derived(points.some((v) => v > 0));
	const maxVal = $derived(Math.max(1, ...points));
</script>

{#if hasActivity}
	<span
		class="spark-bars"
		role="img"
		aria-label="{label}: {points.join(', ')}"
		data-testid="home-throughput-sparkline"
	>
		{#each points as count, idx (idx)}
			<i
				class={['spark-bar', idx === points.length - 1 && 'is-latest']}
				style:height="{Math.max(8, Math.round((count / maxVal) * 100))}%"
				title="{count} closed"
			></i>
		{/each}
	</span>
{/if}

<style>
	.spark-bars {
		display: inline-flex;
		align-items: flex-end;
		gap: 3px;
		height: 24px;
		flex: none;
	}
	.spark-bar {
		display: block;
		width: 6px;
		border-radius: 2px 2px 0 0;
		background: var(--tone-accent-bar, var(--tone-accent));
		opacity: 0.55;
	}
	.spark-bar.is-latest {
		opacity: 1;
	}
</style>
