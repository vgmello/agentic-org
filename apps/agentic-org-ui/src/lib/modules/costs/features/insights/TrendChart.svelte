<!--
  @component
  Spend trend over the selected period (costs-and-insights-ux §1 Charts, §3 By-period output):
  one accessible trend chart (cost per bucket) with labelled values, incomplete/partial bucket indicators
  (never colour-only), and a Chart / Table view toggle. Uses semantic look tokens exclusively.
-->
<script lang="ts">
	import { gutters, mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { formatCost, formatCount, formatTokens, formatUsd } from '../../money';
	import { incompleteText } from '../../present';
	import type { TrendBucket } from '../../types';

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

	const maxMicro = $derived(
		buckets.reduce((max, b) => Math.max(max, b.totals.knownCostMicroUsd ?? 0), 0)
	);
	const chartScale = $derived(maxMicro > 0 ? maxMicro : 1);

	const VIEW_W = 640;
	const VIEW_H = 180;
	const PAD_X = 16;
	const PAD_TOP = 18;
	const PAD_BOTTOM = 28;
	const PLOT_H = VIEW_H - PAD_TOP - PAD_BOTTOM;

	const points = $derived.by(() => {
		const count = buckets.length;
		if (count === 0) return [];
		const usableW = VIEW_W - PAD_X * 2;
		const slotW = usableW / count;
		const barW = Math.min(28, Math.max(1, slotW * 0.62));
		return buckets.map((bucket, idx) => {
			const known = bucket.totals.knownCostMicroUsd !== null;
			const cost = bucket.totals.knownCostMicroUsd ?? 0;
			const ratio = known && maxMicro > 0 ? cost / chartScale : 0;
			const h = known && cost > 0 ? Math.max(3, Math.round(ratio * PLOT_H)) : 0;
			const cx = PAD_X + slotW * idx + slotW / 2;
			const y = PAD_TOP + (PLOT_H - h);
			const incomplete = Boolean(incompleteText(bucket.totals));
			return {
				bucket,
				known,
				cx,
				x: cx - barW / 2,
				barW,
				y,
				h,
				incomplete
			};
		});
	});

	/** Connects consecutive known-cost buckets and restarts with M after unknown (`null`) gaps. */
	const linePath = $derived.by(() => {
		const cmds: string[] = [];
		let open = false;
		for (const pt of points) {
			if (!pt.known) {
				open = false;
				continue;
			}
			cmds.push(`${open ? 'L' : 'M'} ${pt.cx.toFixed(1)} ${pt.y.toFixed(1)}`);
			open = true;
		}
		return cmds.join(' ');
	});

	function showLabel(index: number): boolean {
		const n = buckets.length;
		if (n <= 6 || index === 0 || index === n - 1) return true;
		const step = Math.ceil(n / 5);
		return index % step === 0 && n - 1 - index >= Math.ceil(step / 2);
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
		<div class="mt-4 rounded-lg border border-line-subtle bg-surface-raised/40 p-4">
			<div class="flex flex-wrap items-center justify-between gap-2 text-xs/5 text-fg-muted">
				<span class="tabular-nums">
					Peak bucket: {maxMicro > 0 ? `≈ ${formatUsd(maxMicro)}` : '—'}
				</span>
				<span class="flex flex-wrap items-center gap-x-4 gap-y-1">
					<span class="inline-flex items-center gap-1.5">
						<span aria-hidden="true" class="size-2 rounded-sm bg-brand"></span>
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
				<svg
					viewBox="0 0 {VIEW_W} {VIEW_H}"
					class="h-44 w-full overflow-visible"
					role="img"
					aria-label="{title}: {buckets.length} buckets, peak {maxMicro > 0
						? formatUsd(maxMicro)
						: 'unknown'}. Switch to Table view for exact figures."
					data-testid="trend-svg"
				>
					<line
						x1={PAD_X}
						y1={PAD_TOP + PLOT_H}
						x2={VIEW_W - PAD_X}
						y2={PAD_TOP + PLOT_H}
						class="stroke-line"
						stroke-width="1"
					/>
					<line
						x1={PAD_X}
						y1={PAD_TOP + PLOT_H / 2}
						x2={VIEW_W - PAD_X}
						y2={PAD_TOP + PLOT_H / 2}
						class="stroke-line-subtle"
						stroke-dasharray="3 3"
						stroke-width="1"
					/>
					{#each points as pt (pt.bucket.key)}
						{#if pt.h > 0}
							<rect
								x={pt.x}
								y={pt.y}
								width={pt.barW}
								height={pt.h}
								rx="2"
								class={pt.incomplete || pt.bucket.partial
									? 'fill-warning/40 stroke-warning'
									: 'fill-brand/25 stroke-brand/60'}
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
					{#if linePath}
						<path
							d={linePath}
							fill="none"
							class="stroke-brand"
							stroke-width="2"
							stroke-linejoin="round"
							stroke-linecap="round"
						/>
					{/if}
					{#each points as pt, i (pt.bucket.key)}
						{#if pt.known}
							<circle
								cx={pt.cx}
								cy={pt.y}
								r="3"
								class={pt.incomplete ? 'fill-warning stroke-surface' : 'fill-brand stroke-surface'}
								stroke-width="1.5"
							>
								<title>{pt.bucket.label}: {formatCost(pt.bucket.totals.knownCostMicroUsd)}</title>
							</circle>
						{:else if pt.bucket.totals.records > 0}
							<circle
								cx={pt.cx}
								cy={PAD_TOP + PLOT_H}
								r="3"
								class="fill-surface stroke-warning"
								stroke-dasharray="2 1"
								stroke-width="1.5"
							>
								<title>{pt.bucket.label}: cost unknown (—)</title>
							</circle>
						{/if}
						{#if showLabel(i)}
							<text
								x={pt.cx}
								y={VIEW_H - 6}
								text-anchor={points.length === 1
									? 'middle'
									: i === 0
										? 'start'
										: i === points.length - 1
											? 'end'
											: 'middle'}
								class="fill-fg-muted text-[11px]"
							>
								{pt.bucket.label}{pt.bucket.partial ? '*' : ''}
							</text>
						{/if}
					{/each}
				</svg>
				<figcaption class="mt-2 text-xs/5 text-fg-muted">
					{#if buckets.some((b) => b.partial)}
						* Partial edge bucket clipped by the period bounds or current time.
					{/if}
					Use the Table button for full per-bucket numbers.
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
