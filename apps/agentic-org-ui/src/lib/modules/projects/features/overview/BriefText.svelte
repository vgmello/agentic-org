<!--
  @component
  The project brief as headings, lists and paragraphs: rendered as elements, never as HTML (work-items-ux §10).
  Its headings sit under the Brief heading (h4), so they are h5.
-->
<script lang="ts">
	import { briefBlocks } from '../../present';

	let { markdown }: { markdown: string } = $props();
	const blocks = $derived(briefBlocks(markdown));
</script>

<div class="max-w-prose space-y-3 text-sm/6 text-pretty text-fg-body [overflow-wrap:anywhere]">
	{#each blocks as block, i (i)}
		{#if block.kind === 'heading'}
			<h5 class="pt-2 text-sm/6 font-semibold text-fg first:pt-0">{block.text}</h5>
		{:else if block.kind === 'list'}
			<svelte:element
				this={block.ordered ? 'ol' : 'ul'}
				role="list"
				class="space-y-1 pl-5 marker:text-fg-muted {block.ordered ? 'list-decimal' : 'list-disc'}"
			>
				{#each block.items as item, j (j)}<li>{item}</li>{/each}
			</svelte:element>
		{:else}
			<p>{block.text}</p>
		{/if}
	{/each}
</div>
