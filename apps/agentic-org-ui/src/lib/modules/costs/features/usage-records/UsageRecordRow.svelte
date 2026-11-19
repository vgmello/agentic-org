<!--
  @component
  One effective usage record (costs-and-insights-ux §1 "Usage record row", COST-1): who consumed it and when, the
  runtime and model, the four disjoint token categories (reasoning as part of output), the cost with its basis and the
  exact price version, how its work was attributed, and a correction when it replaced another record. An unknown cost
  reads "—" with its reason. Reused by the task page and the agent profile's Cost tab (plan steps 5 and 7).
-->
<script lang="ts">
	import { formatDayTime } from '../../dates';
	import { formatCost, formatCount } from '../../money';
	import { attributionText, basisText, completenessText, costDashReason } from '../../present';
	import type { UsageRecord } from '../../types';

	let { record, zone }: { record: UsageRecord; zone: string } = $props();

	const consumer = $derived(
		record.consumer.kind === 'agent'
			? `${record.consumer.name}${record.consumer.former ? ' (former)' : ''}`
			: record.consumer.label
	);
	const when = $derived(new Date(record.occurredAt));
	const model = $derived(
		[record.runtime, [record.provider, record.model].filter(Boolean).join('/')]
			.filter(Boolean)
			.join(' · ')
	);
	const tokensText = $derived.by(() => {
		const t = record.tokens;
		if (!t) return 'Tokens not reported';
		const reasoning = t.reasoning ? ` (of which reasoning ${formatCount(t.reasoning)})` : '';
		return `Input ${formatCount(t.input)} · output ${formatCount(t.output)}${reasoning} · cache read ${formatCount(t.cacheRead)} · cache write ${formatCount(t.cacheWrite)}`;
	});
</script>

<div class="flex flex-wrap items-start justify-between gap-x-4 gap-y-1">
	<div class="min-w-0 flex-1 basis-48">
		<p class="text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]">{consumer}</p>
		<p class="text-xs/5 text-fg-muted">
			<time datetime={record.occurredAt}>{formatDayTime(when, zone)}</time> ·
			<span class="[overflow-wrap:anywhere]">{model}</span>
		</p>
	</div>
	<div class="ml-auto text-right">
		<p class="text-sm/6 font-semibold text-fg tabular-nums">
			{#if record.costMicroUsd === null}
				<span aria-hidden="true">—</span><span class="sr-only">{costDashReason(record)}</span>
			{:else}
				{formatCost(record.costMicroUsd)}
			{/if}
		</p>
		<p class="text-xs/5 text-fg-muted">{basisText(record)}</p>
	</div>
</div>
<p class="mt-2 text-xs/5 text-fg-secondary tabular-nums [overflow-wrap:anywhere]">
	{tokensText}
</p>
<p class="mt-1 text-xs/5 text-fg-muted">
	{completenessText(record)}{record.issue ? ` · ${record.issue}` : ''}
</p>
<p class="mt-1 text-xs/5 text-fg-secondary [overflow-wrap:anywhere]">
	{attributionText(record.attribution)}
</p>
{#if record.price}
	<p class="mt-1 text-xs/5 text-fg-muted [overflow-wrap:anywhere]">Price: {record.price.label}</p>
{:else if record.costMicroUsd === null}
	<p class="mt-1 text-xs/5 text-fg-muted">{costDashReason(record)}</p>
{/if}
{#if record.correction}
	<p
		class="mt-1 text-xs/5 text-fg-secondary [overflow-wrap:anywhere]"
		data-testid="record-correction"
	>
		Corrected {formatDayTime(new Date(record.correction.at), zone)} by {record.correction.byName}:
		{record.correction.reason}
	</p>
{/if}
