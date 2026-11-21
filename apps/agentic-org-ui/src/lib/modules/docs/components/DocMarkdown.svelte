<!--
  @component
  Renders a document's Markdown body as safe semantic elements (never HTML), assigning anchor IDs to headings
  so the section rail jumps cleanly to each section.
-->
<script lang="ts">
	import { markdownBlocks } from '../present';

	let {
		body,
		baseLevel = 4,
		idPrefix = 'doc-sec'
	}: { body: string; baseLevel?: 3 | 4; idPrefix?: string } = $props();

	const blocks = $derived(markdownBlocks(body));
</script>

{#if blocks.length === 0}
	<p class="text-sm/6 text-fg-muted">No body text yet.</p>
{:else}
	<div class="flex max-w-prose flex-col gap-3 text-sm/6 text-fg-body [overflow-wrap:anywhere]">
		{#each blocks as block, i (`${i}-${block.kind}`)}
			{#if block.kind === 'heading'}
				{@const level = Math.min(6, baseLevel + Math.max(0, block.level - 2))}
				<svelte:element
					this={`h${level}`}
					id="{idPrefix}-{block.slug}"
					class="scroll-mt-20 pt-2 text-sm/6 font-semibold text-fg first:pt-0"
				>
					{block.text}
				</svelte:element>
			{:else if block.kind === 'list'}
				<svelte:element
					this={block.ordered ? 'ol' : 'ul'}
					role="list"
					class="flex flex-col gap-1 pl-5 marker:text-fg-muted {block.ordered
						? 'list-decimal'
						: 'list-disc'}"
				>
					{#each block.items as item, j (`${i}-${j}`)}
						<li>{item}</li>
					{/each}
				</svelte:element>
			{:else if block.kind === 'code'}
				<pre
					class="overflow-x-auto rounded-md bg-surface-sunken p-3 font-mono text-xs/5 whitespace-pre-wrap text-fg-bright outline-1 -outline-offset-1 outline-line [overflow-wrap:anywhere]">{block.text}</pre>
			{:else}
				<p>{block.text}</p>
			{/if}
		{/each}
	</div>
{/if}
