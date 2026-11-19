<!--
  @component
  A process, standard or skill body as headings, lists and paragraphs: rendered as elements, never as HTML.
  `level` is the heading level of the body's sections.
-->
<script lang="ts">
	import { markdownBlocks } from '../present';

	let { body, level = 4 }: { body: string; level?: 3 | 4 | 5 } = $props();
	const blocks = $derived(markdownBlocks(body));
</script>

{#if blocks.length === 0}
	<p class="text-sm/6 text-fg-muted">No text yet.</p>
{:else}
	<div class="max-w-prose space-y-3 text-sm/6 text-fg-body [overflow-wrap:anywhere]">
		{#each blocks as block, i (i)}
			{#if block.kind === 'heading'}
				<svelte:element this={`h${level}`} class="pt-2 text-sm/6 font-semibold text-fg first:pt-0"
					>{block.text}</svelte:element
				>
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
{/if}
