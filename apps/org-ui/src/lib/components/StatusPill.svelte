<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAttributes } from 'svelte/elements';
	import { Badge } from './ui/badge';
	import { cn } from '$lib/utils';

	let { label, icon, class: className, ...attributes }: Omit<HTMLAttributes<HTMLSpanElement>, 'children'> & {
		label: string;
		icon?: Snippet;
	} = $props();
</script>

<!-- Prototype StatusBadge spacing and inset ring; status meaning belongs to the caller. -->
<Badge variant="outline" class={cn('h-auto min-w-0 shrink gap-x-1.5 overflow-visible rounded-full border-0 bg-gray-400/10 px-2 py-1 text-xs/4 font-medium whitespace-normal text-gray-300 ring-1 ring-gray-400/20 ring-inset', className)} {...attributes}>
	{#if icon}<span class="shrink-0" aria-hidden="true">{@render icon()}</span>{/if}
	<span class="min-w-0 [overflow-wrap:anywhere]">{label}</span>
</Badge>
