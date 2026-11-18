<!--
  @component
  Label/value rows for a record's details and settings pages (Tailwind Plus description list, dark).
  A value is text or a snippet; `mono` sets IDs, paths and hashes in the system mono font, wrapping anywhere.
  An optional `action` snippet per row holds that row's one control (Change, Copy…), right-aligned.
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

	let { items, class: className = '' }: { items: DescriptionItem[]; class?: string } = $props();
</script>

<dl class="divide-y divide-white/5 {className}">
	{#each items as item (item.term)}
		<div
			class="px-4 py-4 min-[640px]:grid min-[640px]:grid-cols-3 min-[640px]:gap-4 min-[640px]:px-6 min-[1024px]:px-8"
		>
			<dt class="text-sm/6 font-medium text-white">{item.term}</dt>
			<dd class="mt-1 flex gap-x-4 text-sm/6 text-gray-400 min-[640px]:col-span-2 min-[640px]:mt-0">
				<span
					class="min-w-0 flex-1 {item.mono
						? 'font-mono [overflow-wrap:anywhere]'
						: 'wrap-break-word'}"
				>
					{#if item.content}{@render item.content()}{:else}{item.value}{/if}
				</span>
				{#if item.action}<span class="shrink-0">{@render item.action()}</span>{/if}
			</dd>
		</div>
	{/each}
</dl>
