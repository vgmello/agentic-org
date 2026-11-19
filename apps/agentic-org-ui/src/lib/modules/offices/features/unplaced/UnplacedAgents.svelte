<!--
  @component
  Agents that aren't placed in an office (§6.4, OFC-10): a list, each with Place… (an office picker) and Offboard…. They don't run
  anywhere until placed.
-->
<script lang="ts">
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import Callout from '$lib/components/Callout.svelte';
	import DividedRow from '$lib/components/DividedRow.svelte';
	import { Button } from '$lib/components/ui/button';
	import type { Member } from '$lib/proto';

	let {
		agents,
		bleed = '',
		gutters = '',
		onplace,
		onoffboard
	}: {
		agents: Member[];
		bleed?: string;
		gutters?: string;
		onplace: (agent: Member, button: HTMLElement) => void;
		onoffboard: (agent: Member, button: HTMLElement) => void;
	} = $props();
</script>

<section aria-labelledby="unplaced" class="space-y-3" data-testid="unplaced-note">
	<h2 id="unplaced" class="text-xs/6 font-semibold text-fg-muted">
		Unplaced · {agents.length}
		{agents.length === 1 ? 'agent' : 'agents'}
	</h2>
	<Callout data-testid="unplaced-explanation">
		{#snippet icon()}<CircleAlert class="size-4 text-warning" aria-hidden="true" />{/snippet}
		<p class="text-sm/6">
			These agents aren't placed in an office, so they don't run anywhere. Place each in an office,
			or offboard it.
		</p>
	</Callout>
	<ul class="{bleed} -mx-4 sm:-mx-6 lg:-mx-8" aria-label="Unplaced agents">
		{#each agents as agent (agent.id)}
			<DividedRow
				class="{gutters} flex flex-wrap items-center justify-between gap-x-3 gap-y-2 text-sm/6 text-fg"
				data-agent={agent.id}
			>
				<span class="min-w-0 [overflow-wrap:anywhere]">{agent.name}</span>
				<span class="flex flex-wrap items-center gap-2">
					<Button
						variant="outline"
						size="sm"
						class="max-[639px]:min-h-11"
						aria-label="Place {agent.name}…"
						onclick={(e) => onplace(agent, e.currentTarget)}>Place…</Button
					>
					<Button
						variant="outline"
						size="sm"
						class="max-[639px]:min-h-11"
						aria-label="Offboard {agent.name}…"
						onclick={(e) => onoffboard(agent, e.currentTarget)}>Offboard…</Button
					>
				</span>
			</DividedRow>
		{/each}
	</ul>
</section>
