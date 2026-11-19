<!--
  @component
  One agent calendar budget in its period (costs-and-insights-ux §1, document 17): its label in the budget's own zone,
  spent of limit in its unit with the percentage, a bar with the 80% mark, and its state in words. Reached waits on the
  owner, so it is the only amber state; an incomplete spend says why and that the figure is a lower bound.
  Reused by the agent profile's Cost tab (plan step 7).
-->
<script lang="ts">
	import {
		budgetAmountText,
		budgetBoundsText,
		budgetPercent,
		budgetPeriodLabel,
		budgetState,
		NO_USAGE_RUNTIME
	} from '../../present';
	import type { BudgetPeriodState } from '../../types';

	let {
		budget,
		kind,
		zone
	}: { budget: BudgetPeriodState; kind: 'daily' | 'monthly'; zone: string } = $props();

	const state = $derived(budgetState(budget));
	const percent = $derived(budgetPercent(budget));
	const label = $derived(budgetPeriodLabel(kind, zone));
	const id = $props.id();
</script>

<div class="min-w-0" data-budget={kind}>
	<div class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
		<p id="{id}-label" class="text-xs/5 font-medium text-fg-secondary [overflow-wrap:anywhere]">
			{label}
		</p>
		{#if state}
			<span
				class="inline-flex max-w-full items-center gap-x-1.5 rounded-md px-2 py-0.5 text-xs/5 font-medium ring-1 ring-inset {state.tone ===
				'owner'
					? 'bg-warning/10 text-warning-text-strong ring-warning/30'
					: 'bg-surface-raised text-fg-secondary ring-line'}"
				data-testid="budget-state"
				><span
					aria-hidden="true"
					class="size-1.5 shrink-0 rounded-full {state.tone === 'owner'
						? 'bg-warning'
						: 'bg-quiet'}"
				></span>{state.word}</span
			>
		{/if}
	</div>
	<div
		role="meter"
		aria-labelledby="{id}-label"
		aria-valuemin={0}
		aria-valuemax={100}
		aria-valuenow={Math.min(percent, 100)}
		aria-valuetext={budgetAmountText(budget)}
		class="relative mt-2 h-2 overflow-hidden rounded-full bg-surface-overlay"
	>
		<span
			class="absolute inset-y-0 left-0 rounded-full {state?.tone === 'owner'
				? 'bg-warning'
				: 'bg-brand'}"
			style:width="{Math.min(percent, 100)}%"
		></span>
		<!-- The 80% mark: the notice threshold. -->
		<span aria-hidden="true" class="absolute inset-y-0 left-4/5 w-px bg-fg-muted"></span>
	</div>
	<p class="mt-1.5 text-xs/5 text-fg-secondary tabular-nums" data-testid="budget-amount">
		{budgetAmountText(budget)}
	</p>
	{#if budget.unmeasured}
		<p class="mt-0.5 text-xs/5 text-fg-muted">{NO_USAGE_RUNTIME}</p>
	{:else if !budget.complete}
		<p class="mt-0.5 text-xs/5 text-fg-muted">
			Usage incomplete{budget.incompleteReason ? `: ${budget.incompleteReason}` : ''}. Spend shown
			is a lower bound.
		</p>
	{/if}
	{#if budget.extended}
		<p class="mt-0.5 text-xs/5 text-fg-muted">This budget already used its one 20% extension.</p>
	{/if}
	<p class="sr-only">{budgetBoundsText(budget, zone)}</p>
</div>
