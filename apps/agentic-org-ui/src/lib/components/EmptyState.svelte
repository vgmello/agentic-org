<!--
  @component
  An empty place (app-shell-ux §6a): what this place is for and one real next action. Not for "no results
  for a filter" (say that in the list, politely announced) and not for an unavailable service.
  Usage:
    <EmptyState title="No projects yet" description="Projects hold the work your teams do.">
      {#snippet action()}<Button variant="outline" onclick={create}>New project</Button>{/snippet}
    </EmptyState>
-->
<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		title,
		description,
		level = 2,
		action,
		class: className = ''
	}: {
		title: string;
		description?: string;
		/** The heading level: 2 for a page, 3 for a section, 4 for a tab under its own h3. */
		level?: 2 | 3 | 4;
		action?: Snippet;
		class?: string;
	} = $props();
</script>

<div class="px-4 py-16 text-center min-[640px]:px-6 min-[1024px]:px-8 {className}">
	<svelte:element this={`h${level}`} class="text-base/7 font-semibold text-white"
		>{title}</svelte:element
	>
	{#if description}<p class="mx-auto mt-1 max-w-md text-sm/6 text-gray-400">{description}</p>{/if}
	{#if action}<div class="mt-6 flex flex-wrap justify-center gap-3">{@render action()}</div>{/if}
</div>
