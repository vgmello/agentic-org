<!--
  @component
  Token usage & cache mix card (chart or table mode) for ProjectInsightsView.
-->
<script lang="ts">
	import { gutters } from '$lib/components/classes';
	import { formatTokens } from '../../money';
	import { cacheShareText } from '../../present';
	import type { UsageTotals } from '../../types';

	let {
		totals,
		displayMode
	}: {
		totals: UsageTotals;
		displayMode: 'chart' | 'table';
	} = $props();

	const tokenMixRows = $derived.by(() => {
		if (totals.knownTokens === null || totals.knownTokens <= 0) return [];
		const total = totals.knownTokens;
		if (totals.cacheReadTokens !== null) {
			const cacheRead = Math.min(total, Math.max(0, totals.cacheReadTokens));
			const nonCache = Math.max(0, total - cacheRead);
			return [
				{
					id: 'non_cache',
					label: 'Input, output & cache write',
					tokens: nonCache,
					pct: Math.round((nonCache * 100) / total),
					tone: 'accent' as const
				},
				{
					id: 'cache_read',
					label: 'Cache read',
					tokens: cacheRead,
					pct: Math.round((cacheRead * 100) / total),
					tone: 'ok' as const
				}
			];
		}
		return [
			{
				id: 'total',
				label: 'Known tokens',
				tokens: total,
				pct: 100,
				tone: 'accent' as const
			}
		];
	});
</script>

{#if tokenMixRows.length > 0}
	<section
		aria-labelledby="token-mix-heading"
		class="border-t border-line-subtle py-6 {gutters}"
		data-testid="token-mix-card"
	>
		<h3 id="token-mix-heading" class="text-base/7 font-semibold text-fg">
			Token usage &amp; cache mix
		</h3>
		<p class="mt-1 text-sm/6 text-fg-muted">
			{formatTokens(totals.knownTokens)} known tokens in this period{#if cacheShareText(totals)}
				· {cacheShareText(totals)}{/if}.
		</p>
		{#if displayMode === 'chart'}
			<ul
				role="list"
				class="mt-4 grid gap-3 rounded-lg border border-line-subtle bg-surface-raised/30 p-4"
			>
				{#each tokenMixRows as row (row.id)}
					<li class="grid grid-cols-1 items-center gap-2 sm:grid-cols-[12rem_minmax(0,1fr)_8rem]">
						<span class="text-sm/6 font-medium text-fg">{row.label}</span>
						<div class="h-2 overflow-hidden rounded-full bg-bar-track">
							<span
								class="block h-full rounded-full"
								style:width="{Math.max(row.tokens > 0 ? 2 : 0, row.pct)}%"
								style:background={row.tone === 'ok'
									? 'var(--tone-ok-bar)'
									: 'var(--tone-accent-bar)'}
							></span>
						</div>
						<span class="text-right text-xs/5 font-semibold text-fg-secondary tabular-nums">
							{formatTokens(row.tokens)} ({row.pct}%)
						</span>
					</li>
				{/each}
			</ul>
		{:else}
			<div class="mt-4 overflow-x-auto rounded-lg border border-line-subtle">
				<table class="w-full text-left text-sm/6" data-testid="token-mix-table">
					<caption class="sr-only">Token usage and cache mix</caption>
					<thead>
						<tr class="border-b border-line text-xs/5 font-semibold text-fg-muted">
							<th scope="col" class="px-4 py-2.5">Category</th>
							<th scope="col" class="px-4 py-2.5 text-right">Tokens</th>
							<th scope="col" class="px-4 py-2.5 text-right">Share</th>
						</tr>
					</thead>
					<tbody class="divide-y divide-line-subtle">
						{#each tokenMixRows as row (row.id)}
							<tr>
								<th scope="row" class="px-4 py-2.5 font-medium text-fg">{row.label}</th>
								<td class="px-4 py-2.5 text-right text-fg-secondary tabular-nums">
									{formatTokens(row.tokens)}
								</td>
								<td class="px-4 py-2.5 text-right text-fg-secondary tabular-nums">
									{row.pct}%
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</section>
{/if}
