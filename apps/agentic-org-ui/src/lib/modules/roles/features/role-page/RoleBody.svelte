<!--
  @component
  A role's Markdown body as headings, lists and paragraphs (roles-ux ROLE-1). Rendered as elements, never as HTML.
  `level` is the heading level of the body's sections under the surrounding heading.
-->
<script lang="ts">
	import { roleBlocks } from '../../present';

	let { body, level = 3 }: { body: string; level?: 3 | 4 } = $props();
	const blocks = $derived(roleBlocks(body));
</script>

{#if blocks.length === 0}
	<p class="text-sm/6 text-gray-400">No text yet.</p>
{:else}
	<div class="max-w-3xl space-y-3 text-sm/6 text-gray-300 [overflow-wrap:anywhere]">
		{#each blocks as block, i (i)}
			{#if block.kind === 'heading'}
				<svelte:element
					this={`h${level}`}
					class="pt-2 text-sm/6 font-semibold text-white first:pt-0">{block.text}</svelte:element
				>
			{:else if block.kind === 'list'}
				<ul role="list" class="list-disc space-y-1 pl-5 marker:text-gray-400">
					{#each block.items as item, j (j)}<li>{item}</li>{/each}
				</ul>
			{:else}
				<p>{block.text}</p>
			{/if}
		{/each}
	</div>
{/if}
