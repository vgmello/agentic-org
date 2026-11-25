<!--
  @component
  Key/value metadata rows for record details and settings panels.
  Each row renders a term, a text value or custom snippet (`mono` for IDs, paths, and hashes),
  and an optional right-aligned action snippet.
  Usage: <DescriptionList items={[{ term: 'Workspace', value: '/Users/ada/org', mono: true }]} />
-->
<script lang="ts" module>
	export interface DescriptionItem {
		term: string;
		value?: string;
		content?: Snippet;
		mono?: boolean;
		action?: Snippet;
	}
</script>

<script lang="ts">
	import type { Snippet } from 'svelte';
	import { cn } from '$lib/utils';

	let { items, class: className = '' }: { items: DescriptionItem[]; class?: string } = $props();
</script>

<dl class={cn('flex flex-col border-t border-line-subtle', className)}>
	{#each items as item (item.term)}
		<div
			class="flex flex-col gap-1 border-b border-line-subtle px-4 py-3.5 last:border-b-0 min-[640px]:grid min-[640px]:grid-cols-[minmax(10rem,1fr)_2fr] min-[640px]:items-baseline min-[640px]:gap-x-6 min-[640px]:px-6 min-[1024px]:px-8"
		>
			<dt class="text-sm leading-6 font-medium text-fg">{item.term}</dt>
			<dd class="flex min-w-0 items-baseline justify-between gap-4 text-sm leading-6 text-fg-muted">
				<span
					class={cn(
						'min-w-0 flex-1',
						item.mono ? 'font-mono [overflow-wrap:anywhere]' : 'wrap-break-word'
					)}
				>
					{#if item.content}{@render item.content()}{:else}{item.value}{/if}
				</span>
				{#if item.action}
					<span class="shrink-0">{@render item.action()}</span>
				{/if}
			</dd>
		</div>
	{/each}
</dl>
