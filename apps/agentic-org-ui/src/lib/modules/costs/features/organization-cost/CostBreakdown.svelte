<!--
  @component
  One breakdown of the organization's cost (costs-and-insights-ux §1 Organization › Cost): a row per project, consumer or
  work type with its cost, tokens and share of the period's known cost. The cost figure opens the records behind it.
  Each row repeats its own "Usage incomplete" qualification. Where the section is wide it is a table (the accessible
  Table view of the share bars); narrower, the same rows stack, never scrolling sideways. Not on a task closes the
  project breakdown, with team and organization overhead kept apart.
-->
<script lang="ts">
	import { formatCost, formatTokens } from '../../money';
	import { incompleteText, overheadText, shareOf } from '../../present';
	import type { BreakdownRow, UsageTotals } from '../../types';

	let {
		rows,
		total,
		caption,
		nameHeader,
		onopen
	}: {
		rows: BreakdownRow[];
		/** The period's totals, for each row's share. */
		total: UsageTotals;
		/** Names the table for assistive technology: "Cost by project, This month". */
		caption: string;
		nameHeader: string;
		onopen: (row: BreakdownRow, opener: HTMLElement) => void;
	} = $props();

	const share = (row: BreakdownRow) =>
		shareOf(row.totals.knownCostMicroUsd, total.knownCostMicroUsd);

	const openLabel = (row: BreakdownRow) =>
		row.totals.knownCostMicroUsd === null
			? `Cost unknown. Open the records for ${row.label}`
			: `${formatCost(row.totals.knownCostMicroUsd)}. Open the records for ${row.label}`;

	const figureClass =
		'inline-flex min-h-6 items-center rounded-sm font-semibold text-link tabular-nums hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11';
</script>

{#snippet bar(row: BreakdownRow)}
	{@const pct = share(row)}
	<span class="flex items-center gap-x-3">
		<span
			aria-hidden="true"
			class="h-2 min-w-12 flex-1 overflow-hidden rounded-full bg-surface-overlay"
		>
			<span class="block h-full rounded-full bg-brand" style:width="{pct ?? 0}%"></span>
		</span>
		<span class="w-10 shrink-0 text-right text-sm/6 text-fg-secondary tabular-nums"
			>{pct === undefined ? '—' : `${pct}%`}</span
		>
	</span>
{/snippet}

{#snippet meta(row: BreakdownRow)}
	{#if row.overhead}<p class="mt-1 text-xs/5 text-fg-muted">{overheadText(row.overhead)}</p>{/if}
	{#if incompleteText(row.totals)}
		<p class="mt-1 text-xs/5 text-fg-muted" data-testid="row-incomplete">
			{incompleteText(row.totals)}
		</p>
	{/if}
{/snippet}

<div class="@container">
	<table class="hidden w-full text-sm/6 @3xl:table" data-testid="breakdown-table">
		<caption class="sr-only">{caption}</caption>
		<thead>
			<tr class="border-b border-line text-left text-xs/5 font-semibold text-fg-muted">
				<th scope="col" class="py-3 pr-4 pl-4 min-[640px]:pl-6 min-[1024px]:pl-8">{nameHeader}</th>
				<th scope="col" class="w-32 px-4 py-3 text-right">Cost (USD)</th>
				<th scope="col" class="w-28 px-4 py-3 text-right">Tokens</th>
				<th scope="col" class="w-56 py-3 pr-4 pl-4 min-[640px]:pr-6 min-[1024px]:pr-8"
					>Share of cost</th
				>
			</tr>
		</thead>
		<tbody>
			{#each rows as row (row.id)}
				<tr
					class="border-b border-line-subtle align-top {row.overhead
						? 'border-t border-t-line'
						: ''}"
					data-row={row.id}
				>
					<th
						scope="row"
						class="py-4 pr-4 pl-4 text-left font-normal min-[640px]:pl-6 min-[1024px]:pl-8"
					>
						<span class="font-semibold text-fg [overflow-wrap:anywhere]">{row.label}</span
						>{#if row.archived}<span class="ml-2 text-xs/5 text-fg-muted">archived</span>{/if}
						{@render meta(row)}
					</th>
					<td class="px-4 py-4 text-right">
						<button
							type="button"
							class={figureClass}
							aria-label={openLabel(row)}
							onclick={(e) => onopen(row, e.currentTarget)}
							>{formatCost(row.totals.knownCostMicroUsd)}</button
						>
					</td>
					<td class="px-4 py-4 text-right text-fg-secondary tabular-nums"
						>{formatTokens(row.totals.knownTokens)}</td
					>
					<td class="py-4 pr-4 pl-4 min-[640px]:pr-6 min-[1024px]:pr-8">{@render bar(row)}</td>
				</tr>
			{/each}
		</tbody>
	</table>

	<ul role="list" class="divide-y divide-line-subtle border-y border-line-subtle @3xl:hidden">
		{#each rows as row (row.id)}
			<li
				class="px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8 {row.overhead
					? 'border-t border-line'
					: ''}"
				data-row={row.id}
			>
				<div class="flex flex-wrap items-start justify-between gap-x-4 gap-y-1">
					<p
						class="min-w-0 flex-1 basis-40 text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]"
					>
						{row.label}{#if row.archived}<span class="ml-2 text-xs/5 font-normal text-fg-muted"
								>archived</span
							>{/if}
					</p>
					<button
						type="button"
						class="text-sm/6 {figureClass}"
						aria-label={openLabel(row)}
						onclick={(e) => onopen(row, e.currentTarget)}
						>{formatCost(row.totals.knownCostMicroUsd)}</button
					>
				</div>
				<p class="mt-1 text-xs/5 text-fg-muted tabular-nums">
					{formatTokens(row.totals.knownTokens)} tokens
				</p>
				<div class="mt-2">{@render bar(row)}</div>
				{@render meta(row)}
			</li>
		{/each}
	</ul>
</div>
