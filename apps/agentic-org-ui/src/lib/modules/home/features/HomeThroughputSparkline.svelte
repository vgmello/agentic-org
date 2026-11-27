<script lang="ts">
	import { smoothAreaPath, smoothCurvePath } from '$lib/components/chart-math';

	let {
		points,
		fallbackPct = 0,
		label = '7-day completed throughput'
	}: {
		points: readonly number[];
		fallbackPct?: number;
		label?: string;
	} = $props();

	const gradId = $props.id();
	const W = 220;
	const H = 26;
	const PAD_Y = 3;

	const hasActivity = $derived(points.some((v) => v > 0));
	const maxVal = $derived(Math.max(1, ...points));

	const coords = $derived.by(() => {
		const n = Math.max(2, points.length);
		const stepX = W / (n - 1);
		return points.map((v, idx) => ({
			x: idx * stepX,
			y: H - PAD_Y - (v / maxVal) * (H - PAD_Y * 2)
		}));
	});

	const lineD = $derived(smoothCurvePath(coords));
	const areaD = $derived(smoothAreaPath(coords, H));
	const lastPt = $derived(coords[coords.length - 1]);
</script>

{#if hasActivity && coords.length >= 2}
	<div class="spark-wrap flex items-center gap-2" data-testid="home-throughput-sparkline">
		<svg
			viewBox="0 0 {W} {H}"
			class="h-6 w-full overflow-visible"
			role="img"
			aria-label="{label}: {points.join(', ')}"
		>
			<defs>
				<linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
					<stop offset="0%" stop-color="var(--tone-ok-ink)" stop-opacity="0.32" />
					<stop offset="100%" stop-color="var(--tone-ok-ink)" stop-opacity="0" />
				</linearGradient>
			</defs>
			{#if areaD}
				<path d={areaD} fill="url(#{gradId})" />
			{/if}
			{#if lineD}
				<path
					d={lineD}
					fill="none"
					class="spark-line"
					stroke-width="1.75"
					stroke-linecap="round"
					stroke-linejoin="round"
				/>
			{/if}
			{#if lastPt}
				<circle cx={lastPt.x} cy={lastPt.y} r="2.25" class="spark-dot" />
			{/if}
		</svg>
	</div>
{:else}
	<div class="bar-track bg-bar-track" aria-hidden="true">
		<span class="bar-ok" style:width="{fallbackPct}%"></span>
	</div>
{/if}

<style>
	.spark-line {
		stroke: var(--tone-ok-ink);
	}
	.spark-dot {
		fill: var(--tone-ok-ink);
	}
	.bar-track {
		display: flex;
		width: 100%;
		height: 5px;
		overflow: hidden;
		border-radius: 999px;
	}
	.bar-ok {
		background: var(--exec-bar-ok);
		border-radius: 999px;
	}
</style>
