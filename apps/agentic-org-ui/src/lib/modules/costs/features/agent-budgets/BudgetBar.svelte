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
		extensionText,
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
	const fillTone = $derived<'bad' | 'warn' | 'accent'>(
		budget.spent >= budget.limit || state?.tone === 'owner'
			? 'bad'
			: percent >= 80
				? 'warn'
				: 'accent'
	);
	const id = $props.id();
</script>

<div class="min-w-0" data-budget={kind}>
	<div class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
		<p id="{id}-label" class="text-xs/5 font-medium text-fg-secondary [overflow-wrap:anywhere]">
			{label}
		</p>
		{#if state}
			<span
				class={[
					'inline-flex max-w-full items-center gap-x-1.5 rounded-full px-2 py-0.5 text-xs/5 font-medium',
					state.tone === 'owner'
						? 't-amber'
						: 'bg-surface-raised text-fg-secondary ring-1 ring-line ring-inset'
				]}
				data-testid="budget-state"
				><span
					aria-hidden="true"
					class={[
						'size-1.5 shrink-0 rounded-full',
						state.tone === 'owner' ? 'budget-dot-warn' : 'bg-quiet'
					]}
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
		class="relative mt-2 h-2 overflow-hidden rounded-full bg-bar-track"
	>
		<span
			class={['budget-fill absolute inset-y-0 left-0 rounded-full', `budget-fill-${fillTone}`]}
			style:width="{Math.min(percent, 100)}%"
		></span>
		<!-- The 80% mark: the notice threshold. -->
		<span aria-hidden="true" class="absolute inset-y-0 left-4/5 w-px bg-fg-muted/70"></span>
	</div>
	<p
		class="mt-1.5 text-xs/5 font-medium text-fg-secondary tabular-nums"
		data-testid="budget-amount"
	>
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
		<p class="mt-0.5 text-xs/5 text-fg-muted">
			This budget already used its one 20% extension{budget.baseLimit !== undefined
				? `: ${extensionText(budget)}`
				: ''}.
		</p>
	{/if}
	<!-- §4: the period's bounds in the budget zone, inclusive start and exclusive end, shown, not only spoken. -->
	<p class="mt-0.5 text-xs/5 text-fg-muted tabular-nums" data-testid="budget-bounds">
		{budgetBoundsText(budget, zone)}
	</p>
</div>

<style>
	.budget-fill-accent {
		background: var(--tone-accent-bar);
	}
	.budget-fill-warn {
		background: var(--tone-warn-bar);
	}
	.budget-fill-bad {
		background: var(--tone-bad-bar);
	}
	.budget-dot-warn {
		background: var(--tone-warn);
	}
</style>
