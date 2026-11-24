<!--
  @component
  Budget state chip on work rows, cards and state-only Cost blocks (costs-and-insights-ux §1, §4).
  Shows budget state in plain words without revealing amounts; amber is used only when reached.
-->
<script lang="ts">
	import { tone } from '$lib/components/pills';
	import { budgetChipText, budgetTone } from '../budget';
	import type { BudgetWord } from '../types';
	import WorkIcon from './WorkIcon.svelte';

	let {
		budget,
		hold
	}: {
		budget: BudgetWord;
		hold?: 'stop_pending' | 'held';
	} = $props();

	const label = $derived(budgetChipText(budget, hold));
	const toneClass = $derived(tone[budgetTone(budget)]);
</script>

<span
	class="inline-flex max-w-full items-center gap-x-1 rounded-md px-2 py-0.5 text-xs/5 font-medium ring-1 ring-inset {toneClass ||
		'bg-surface-overlay text-fg-secondary ring-line'}"
	data-testid="budget-chip"
	data-budget={budget}
>
	<WorkIcon name="budget" class="size-3.5" />
	<span class="truncate">{label}</span>
</span>
