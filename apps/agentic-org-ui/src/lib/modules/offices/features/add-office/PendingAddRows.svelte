<!--
  @component
  An Add office that waits for running agents to stop (§6.2, OFC-37): not an office yet, so it has no status dot, no agents and no
  other action than Cancel. Choosing it reopens the form in its Waiting state.
-->
<script lang="ts">
	import Hourglass from '@lucide/svelte/icons/hourglass';
	import { Button } from '$lib/components/ui/button';
	import { plural } from '../../copy';
	import type { PendingAdd } from '../../types';

	let {
		pending,
		onopen,
		oncancel
	}: {
		pending: PendingAdd[];
		onopen: (add: PendingAdd, button: HTMLElement) => void;
		oncancel: (add: PendingAdd, button: HTMLElement) => void;
	} = $props();
</script>

{#each pending as add (add.id)}
	<li
		class="relative flex flex-wrap items-center justify-between gap-x-4 gap-y-3 border-b border-line-subtle px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8"
		data-testid="pending-add"
	>
		<button
			type="button"
			class="min-w-0 flex-1 basis-56 text-left outline-none [&:focus-visible>.cover]:outline-2 [&:focus-visible>.cover]:outline-focus [&:focus-visible>.cover]:-outline-offset-2"
			aria-label="{add.name}, waiting for {plural(
				add.waitingFor.length,
				'agent',
				'agents'
			)} to stop. Opens the form."
			onclick={(e) => onopen(add, e.currentTarget)}
		>
			<span
				class="flex items-center gap-x-2 text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]"
				><Hourglass class="size-4 shrink-0 text-fg-muted" aria-hidden="true" />{add.name}</span
			>
			<span class="mt-1 block text-xs/5 text-fg-muted"
				>Waiting for {plural(add.waitingFor.length, 'agent', 'agents')} to stop · on {add.instance}</span
			>
			<span class="cover absolute inset-0" aria-hidden="true"></span>
		</button>
		<Button
			variant="outline"
			size="sm"
			class="relative z-10 max-[639px]:min-h-11"
			aria-label="Cancel adding {add.name}"
			onclick={(e) => oncancel(add, e.currentTarget)}>Cancel</Button
		>
	</li>
{/each}
