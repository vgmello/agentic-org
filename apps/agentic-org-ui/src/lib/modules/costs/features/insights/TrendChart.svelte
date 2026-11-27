<!--
  @component
  Spend trend over the selected period (costs-and-insights-ux §1 Charts, §3 By-period output; TASK-193 / TASK-229):
  accessible gradient SVG area + Bezier curve + subtle bucket bars, clean Y-axis ticks & hairline gridlines,
  roving-tabindex arrow-key focus points with hover/focus readout, incomplete/partial bucket indicators
  (never colour-only), and a Chart / Table twin toggle across all 7 themes.
-->
<script lang="ts">
	import { gutters, mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { formatCost, formatCount, formatTokens, formatUsd } from '../../money';
	import { incompleteText } from '../../present';
	import type { TrendBucket } from '../../types';
	import {
		axisLevel,
		contiguousRuns,
		niceTicks,
		smoothAreaPath,
		smoothCurvePath
	} from '$lib/components/chart-math';

	let {
		buckets,
		title = 'Spend trend',
		description = 'Known API-equivalent cost per bucket across the selected period.',
		mode = $bindable<'chart' | 'table'>('chart')
	}: {
		buckets: TrendBucket[];
		title?: string;
		description?: string;
		mode?: 'chart' | 'table';
	} = $props();

	const headingId = $props.id();
	const gradId = $derived(`${headingId}-area-grad`);

	const maxMicro = $derived(
		buckets.reduce((max, b) => Math.max(max, b.totals.knownCostMicroUsd ?? 0), 0)
	);
	const maxUsd = $derived(maxMicro / 1_000_000);
	const yTicksUsd = $derived(niceTicks(maxUsd));
	const yTopMicro = $derived(Math.max(1, (yTicksUsd[yTicksUsd.length - 1] ?? 1) * 1_000_000));

	const VIEW_W = 640;
	const VIEW_H = 196;
	const PAD_LEFT = 54;
	const PAD_RIGHT = 14;
	const PAD_TOP = 16;
	const PAD_BOTTOM = 28;
	const PLOT_W = VIEW_W - PAD_LEFT - PAD_RIGHT;
	const PLOT_H = VIEW_H - PAD_TOP - PAD_BOTTOM;
	const BASELINE_Y = PAD_TOP + PLOT_H;

	const points = $derived.by(() => {
		const count = buckets.length;
		if (count === 0) return [];
		const slotW = PLOT_W / count;
		const barW = Math.min(24, Math.max(2, slotW * 0.46));
		return buckets.map((bucket, idx) => {
			const known = bucket.totals.knownCostMicroUsd !== null;
			const cost = bucket.totals.knownCostMicroUsd ?? 0;
			const ratio = known && maxMicro > 0 ? Math.min(1, cost / yTopMicro) : 0;
			const h = known && cost > 0 ? Math.max(3, Math.round(ratio * PLOT_H)) : 0;
			const cx = PAD_LEFT + slotW * idx + slotW / 2;
			const y = BASELINE_Y - ratio * PLOT_H;
			const inc = incompleteText(bucket.totals);
			return {
				index: idx,
				bucket,
				known,
				cx,
				x: cx - barW / 2,
				barW,
				y,
				h,
				incomplete: Boolean(inc),
				incompleteNote: inc,
				axisVisibility: axisLevel(idx, count)
			};
		});
	});

	const curveRuns = $derived(contiguousRuns(points, (pt) => pt.known));
	const curvePaths = $derived(
		curveRuns.map((run) => ({
			line: smoothCurvePath(run.map((pt) => ({ x: pt.cx, y: pt.y }))),
			area: smoothAreaPath(
				run.map((pt) => ({ x: pt.cx, y: pt.y })),
				BASELINE_Y
			)
		}))
	);

	let activeIdx = $state<number | null>(null);
	let focusedIdx = $state<number | null>(null);
	let hitsListEl = $state<HTMLOListElement | null>(null);
	const effectiveActiveIdx = $derived(
		activeIdx !== null && activeIdx < points.length ? activeIdx : null
	);
	const activePoint = $derived(
		effectiveActiveIdx !== null ? points[effectiveActiveIdx] : (points[points.length - 1] ?? null)
	);

	function formatTickUsd(usd: number): string {
		if (usd === 0) return '$0';
		return formatUsd(Math.round(usd * 1_000_000));
	}

	function handlePointKeydown(event: KeyboardEvent, idx: number) {
		const count = points.length;
		if (count <= 1) return;
		const nextMap: Record<string, number | undefined> = {
			ArrowLeft: Math.max(0, idx - 1),
			ArrowRight: Math.min(count - 1, idx + 1),
			Home: 0,
			End: count - 1
		};
		const next = nextMap[event.key];
		if (next === undefined || next === idx) return;
		event.preventDefault();
		activeIdx = next;
		focusedIdx = next;
		const el = hitsListEl?.querySelector<HTMLButtonElement>(`[data-point-idx="${next}"]`);
		el?.focus();
	}
</script>

<section
	aria-labelledby={headingId}
	class="border-t border-line-subtle py-6 {gutters}"
	data-testid="spend-trend"
>
	<div class="flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
		<div class="min-w-0 flex-1 basis-60">
			<h3 id={headingId} class="text-base/7 font-semibold text-fg">{title}</h3>
			<p class="mt-1 text-sm/6 text-fg-muted">{description}</p>
		</div>
		{#if buckets.length > 0}
			<div class="flex items-center gap-2" role="group" aria-label="Trend display mode">
				<Button
					type="button"
					variant={mode === 'chart' ? 'secondary' : 'outline'}
					size="sm"
					class={mobileButton}
					aria-pressed={mode === 'chart'}
					onclick={() => (mode = 'chart')}>Chart</Button
				>
				<Button
					type="button"
					variant={mode === 'table' ? 'secondary' : 'outline'}
					size="sm"
					class={mobileButton}
					aria-pressed={mode === 'table'}
					onclick={() => (mode = 'table')}>Table</Button
				>
			</div>
		{/if}
	</div>

	{#if buckets.length === 0}
		<p class="mt-4 text-sm/6 text-fg-muted">No spend buckets recorded in this period.</p>
	{:else if mode === 'chart'}
		<div class="trend-shell mt-4 rounded-xl border border-line-subtle bg-surface-raised/40 p-4">
			<div class="flex flex-wrap items-center justify-between gap-2 text-xs/5 text-fg-muted">
				<div class="flex flex-wrap items-center gap-x-4 gap-y-1">
					<span class="tabular-nums">
						Peak: <strong class="font-semibold text-fg"
							>{maxMicro > 0 ? `≈ ${formatUsd(maxMicro)}` : '—'}</strong
						>
					</span>
					{#if activePoint}
						<span
							class="trend-readout inline-flex items-center gap-1.5 rounded-md px-2 py-0.5 tabular-nums"
							data-testid="trend-readout"
						>
							<span class="font-semibold text-fg"
								>{activePoint.bucket.label}{activePoint.bucket.partial ? ' (partial)' : ''}:</span
							>
							<span class="font-semibold text-fg"
								>{formatCost(activePoint.bucket.totals.knownCostMicroUsd)}</span
							>
							<span>· {formatTokens(activePoint.bucket.totals.knownTokens)} tokens</span>
							<span>· {formatCount(activePoint.bucket.totals.records)} rec</span>
						</span>
					{/if}
				</div>
				<span class="flex flex-wrap items-center gap-x-4 gap-y-1">
					<span class="inline-flex items-center gap-1.5">
						<span aria-hidden="true" class="trend-legend-swatch is-accent"></span>
						Known cost
					</span>
					<span class="inline-flex items-center gap-1.5">
						<span
							aria-hidden="true"
							class="size-2 rounded-sm border border-dashed border-warning bg-warning/30"
						></span>
						Incomplete or partial bucket
					</span>
				</span>
			</div>

			<figure class="mt-3">
				<div class="trend-plot-wrap relative">
					<svg
						viewBox="0 0 {VIEW_W} {VIEW_H}"
						class="h-48 w-full overflow-visible"
						role="img"
						aria-label="{title}: {buckets.length} buckets, peak {maxMicro > 0
							? formatUsd(maxMicro)
							: 'unknown'}. Arrow keys inspect points; switch to Table view for exact figures."
						data-testid="trend-svg"
					>
						<defs>
							<linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
								<stop offset="0%" stop-color="var(--chart-accent)" stop-opacity="0.32" />
								<stop offset="100%" stop-color="var(--chart-accent)" stop-opacity="0" />
							</linearGradient>
						</defs>

						{#each yTicksUsd as tickVal (tickVal)}
							{@const ratio = yTopMicro > 0 ? (tickVal * 1_000_000) / yTopMicro : 0}
							{@const gy = BASELINE_Y - ratio * PLOT_H}
							<line
								x1={PAD_LEFT}
								y1={gy}
								x2={VIEW_W - PAD_RIGHT}
								y2={gy}
								class={tickVal === 0 ? 'trend-axis-base' : 'trend-gridline'}
								stroke-width="1"
							/>
							<text
								x={PAD_LEFT - 8}
								y={gy + 3.5}
								text-anchor="end"
								class="trend-axis-text text-[11px] tabular-nums"
							>
								{formatTickUsd(tickVal)}
							</text>
						{/each}

						{#each points as pt (pt.bucket.key)}
							{#if pt.h > 0}
								<rect
									x={pt.x}
									y={BASELINE_Y - pt.h}
									width={pt.barW}
									height={pt.h}
									rx="3"
									class={[
										pt.incomplete || pt.bucket.partial ? 'trend-bar-warn' : 'trend-bar',
										effectiveActiveIdx === pt.index && 'is-active'
									]}
									stroke-width="1"
									stroke-dasharray={pt.bucket.partial ? '2 2' : undefined}
								>
									<title
										>{pt.bucket.label}{pt.bucket.partial ? ' (partial)' : ''}: {formatCost(
											pt.bucket.totals.knownCostMicroUsd
										)} · {formatTokens(pt.bucket.totals.knownTokens)} tokens</title
									>
								</rect>
							{/if}
						{/each}

						{#each curvePaths as seg, segIdx (segIdx)}
							{#if seg.area}
								<path d={seg.area} fill="url(#{gradId})" />
							{/if}
							{#if seg.line}
								<path
									d={seg.line}
									fill="none"
									class="trend-curve"
									stroke-width="2.25"
									stroke-linejoin="round"
									stroke-linecap="round"
								/>
							{/if}
						{/each}

						{#each points as pt (pt.bucket.key)}
							{@const isActive = effectiveActiveIdx === pt.index}
							{#if pt.known}
								<circle
									cx={pt.cx}
									cy={pt.y}
									r={isActive ? 4.5 : 3}
									class={[
										pt.incomplete ? 'fill-warning stroke-surface' : 'trend-dot',
										isActive && 'is-active'
									]}
									stroke-width="1.75"
								/>
							{:else if pt.bucket.totals.records > 0}
								<circle
									cx={pt.cx}
									cy={BASELINE_Y}
									r={isActive ? 4.5 : 3}
									class="fill-surface stroke-warning"
									stroke-dasharray="2 1"
									stroke-width="1.5"
								/>
							{/if}
							{#if pt.axisVisibility !== 'none'}
								<text
									x={pt.cx}
									y={VIEW_H - 6}
									text-anchor={points.length === 1
										? 'middle'
										: pt.index === 0
											? 'start'
											: pt.index === points.length - 1
												? 'end'
												: 'middle'}
									class="trend-axis-text text-[11px]"
									data-axis-level={pt.axisVisibility}
								>
									{pt.bucket.label}{pt.bucket.partial ? '*' : ''}
								</text>
							{/if}
						{/each}
					</svg>

					<!-- Accessible roving-tabindex hit overlay (arrow keys inspect buckets; hover/focus share readout) -->
					<ol
						bind:this={hitsListEl}
						class="trend-hits"
						aria-label="{title} data points. {points.length} buckets; use Left and Right arrow keys to inspect."
						style:--plot-left="{(PAD_LEFT / VIEW_W) * 100}%"
						style:--plot-right="{(PAD_RIGHT / VIEW_W) * 100}%"
						style:--n={points.length}
					>
						{#each points as pt (pt.bucket.key)}
							{@const tabStop =
								focusedIdx !== null ? focusedIdx === pt.index : pt.index === points.length - 1}
							<li class="trend-hit-cell">
								<button
									type="button"
									data-point-idx={pt.index}
									tabindex={tabStop ? 0 : -1}
									class="trend-hit-btn"
									aria-label="{pt.bucket.label}{pt.bucket.partial ? ' (partial)' : ''}: {formatCost(
										pt.bucket.totals.knownCostMicroUsd
									)}, {formatTokens(pt.bucket.totals.knownTokens)} tokens, {formatCount(
										pt.bucket.totals.records
									)} records"
									onpointerenter={() => (activeIdx = pt.index)}
									onpointerleave={() => {
										if (focusedIdx === null) activeIdx = null;
									}}
									onfocus={() => {
										activeIdx = pt.index;
										focusedIdx = pt.index;
									}}
									onblur={() => {
										if (focusedIdx === pt.index) focusedIdx = null;
									}}
									onkeydown={(e) => handlePointKeydown(e, pt.index)}
								></button>
							</li>
						{/each}
					</ol>
				</div>

				<figcaption class="mt-2 text-xs/5 text-fg-muted">
					{#if buckets.some((b) => b.partial)}
						* Partial edge bucket clipped by the period bounds or current time.
					{/if}
					Use arrow keys on the chart to inspect buckets, or switch to Table for full figures.
				</figcaption>
			</figure>
		</div>
	{:else}
		<div class="mt-4 overflow-x-auto rounded-lg border border-line-subtle">
			<table class="w-full text-left text-sm/6" data-testid="trend-table">
				<caption class="sr-only">{title} values by bucket</caption>
				<thead>
					<tr class="border-b border-line text-xs/5 font-semibold text-fg-muted">
						<th scope="col" class="px-4 py-2.5">Bucket</th>
						<th scope="col" class="px-4 py-2.5 text-right">Cost (USD)</th>
						<th scope="col" class="px-4 py-2.5 text-right">Tokens</th>
						<th scope="col" class="px-4 py-2.5 text-right">Records</th>
						<th scope="col" class="px-4 py-2.5">Notes</th>
					</tr>
				</thead>
				<tbody class="divide-y divide-line-subtle">
					{#each buckets as b (b.key)}
						{@const inc = incompleteText(b.totals)}
						<tr>
							<th scope="row" class="px-4 py-2.5 font-medium text-fg">
								{b.label}{#if b.partial}
									<span class="ml-1.5 text-xs/5 font-normal text-fg-muted">(partial)</span>
								{/if}
							</th>
							<td class="px-4 py-2.5 text-right font-medium text-fg tabular-nums">
								{formatCost(b.totals.knownCostMicroUsd)}
							</td>
							<td class="px-4 py-2.5 text-right text-fg-secondary tabular-nums">
								{formatTokens(b.totals.knownTokens)}
							</td>
							<td class="px-4 py-2.5 text-right text-fg-secondary tabular-nums">
								{formatCount(b.totals.records)}
							</td>
							<td class="px-4 py-2.5 text-xs/5 text-fg-muted">
								{inc ?? (b.partial ? 'Partial edge bucket' : 'Complete')}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}
</section>

<style>
	.trend-legend-swatch.is-accent {
		width: 10px;
		height: 3px;
		border-radius: 999px;
		background: var(--chart-accent);
	}
	.trend-readout {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.trend-axis-base {
		stroke: var(--exec-border-hi);
	}
	.trend-gridline {
		stroke: var(--chart-grid);
		stroke-dasharray: 2 3;
	}
	.trend-axis-text {
		fill: var(--chart-axis);
	}
	.trend-bar {
		fill: color-mix(in srgb, var(--chart-tint) 18%, transparent);
		stroke: color-mix(in srgb, var(--chart-accent) 42%, transparent);
	}
	.trend-bar.is-active {
		fill: color-mix(in srgb, var(--chart-accent) 32%, transparent);
		stroke: var(--chart-accent);
	}
	.trend-bar-warn {
		fill: var(--tone-warn-bg);
		stroke: var(--tone-warn);
	}
	.trend-curve {
		stroke: var(--chart-accent);
	}
	.trend-dot {
		fill: var(--chart-accent);
		stroke: var(--exec-surface);
	}
	.trend-dot.is-active {
		stroke-width: 2.25;
	}
	.trend-hits {
		position: absolute;
		inset-block: 0 24px;
		inset-inline: var(--plot-left) var(--plot-right);
		display: grid;
		grid-template-columns: repeat(var(--n, 1), minmax(0, 1fr));
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.trend-hit-cell {
		display: flex;
		min-width: 0;
	}
	.trend-hit-btn {
		flex: 1;
		background: transparent;
		border: 0;
		cursor: crosshair;
		border-radius: 4px;
	}
	.trend-hit-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	@media (max-width: 520px) {
		.trend-axis-text[data-axis-level='many'] {
			display: none;
		}
	}
</style>
