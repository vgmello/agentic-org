<!--
  @component
  Guards an agent-only tab (Runs, Processes, Role, Notes and memory, Cost): a person's profile has no such tab, so a
  typed or old link says so and links back to the Overview instead of showing an agent's empty state.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import { resolve } from '$app/paths';
	import { profileContext } from '../profile/context.svelte';
	import { tabHref } from '../profile/tabs';

	let { what, children }: { what: string; children: Snippet } = $props();
	const ctx = profileContext();
</script>

{#if ctx.profile.kind === 'agent'}
	{@render children()}
{:else}
	<EmptyState
		level={3}
		title="People don't have {what}"
		description="This tab is for agents. {ctx.profile.name} is a person."
	>
		{#snippet action()}
			<a
				href={resolve(tabHref(ctx.profile.id, 'overview') as '/')}
				class="rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
				>Open {ctx.profile.name}'s overview</a
			>
		{/snippet}
	</EmptyState>
{/if}
