<!--
  @component
  Key/value metadata rows for record details and settings panels.
  Each row renders a term, a text value or custom snippet (`mono` for IDs, paths, and hashes),
  and an optional right-aligned action snippet.
  Usage: <DescriptionList items={[{ term: 'Workspace', value: '/Users/ada/org', mono: true }]} />
-->
<script lang="ts" module>
	export interface DescriptionItem {
		id?: string;
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

<dl class={cn('divide-y divide-line-subtle', className)}>
	{#each items as item (item.id ?? item.term)}
		<div
			class="flex flex-col gap-1 px-4 py-4 min-[640px]:grid min-[640px]:grid-cols-3 min-[640px]:items-baseline min-[640px]:gap-x-6 min-[640px]:px-6 min-[1024px]:px-8"
		>
			<dt class="min-w-0 text-sm/6 font-medium wrap-break-word text-fg">{item.term}</dt>
			{#if item.action}
				<dd
					class="flex min-w-0 flex-wrap items-baseline justify-between gap-x-4 gap-y-2 text-sm/6 text-fg-muted min-[640px]:col-span-2"
				>
					<div
						class={cn(
							'min-w-0 flex-1 basis-40',
							item.mono ? 'font-mono [overflow-wrap:anywhere]' : 'wrap-break-word'
						)}
					>
						{#if item.content}{@render item.content()}{:else}{item.value}{/if}
					</div>
					<div class="ms-auto shrink-0">{@render item.action()}</div>
				</dd>
			{:else}
				<dd
					class={cn(
						'min-w-0 text-sm/6 text-fg-muted min-[640px]:col-span-2',
						item.mono ? 'font-mono [overflow-wrap:anywhere]' : 'wrap-break-word'
					)}
				>
					{#if item.content}{@render item.content()}{:else}{item.value}{/if}
				</dd>
			{/if}
		</div>
	{/each}
</dl>
