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
		canplace = true,
		onplace,
		onoffboard
	}: {
		agents: Member[];
		bleed?: string;
		gutters?: string;
		/** Placing goes through the offices API; when it isn't served, only Offboard… is offered. */
		canplace?: boolean;
		onplace: (agent: Member, button: HTMLElement) => void;
		onoffboard: (agent: Member, button: HTMLElement) => void;
	} = $props();
</script>

<section aria-labelledby="unplaced" class="space-y-3" data-testid="unplaced-note">
	<h3
		id="unplaced"
		tabindex="-1"
		class="text-xs/6 font-semibold text-fg-muted outline-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
	>
		Unplaced · {agents.length}
		{agents.length === 1 ? 'agent' : 'agents'}
	</h3>
	<Callout data-testid="unplaced-explanation">
		{#snippet icon()}<CircleAlert class="size-4 text-warning" aria-hidden="true" />{/snippet}
		<p class="text-sm/6">
			These agents aren't placed in an office, so they don't run anywhere. {canplace
				? 'Place each in an office, or offboard it.'
				: "Placing them in an office isn't built yet; you can offboard them."}
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
					{#if canplace}<Button
							variant="outline"
							size="sm"
							class="max-[639px]:min-h-11"
							aria-label="Place {agent.name}…"
							onclick={(e) => onplace(agent, e.currentTarget)}>Place…</Button
						>{/if}
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
