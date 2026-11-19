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
<Badge variant="outline" class={cn('h-auto flex-none max-w-full gap-x-1.5 overflow-visible rounded-full border-0 bg-quiet/10 px-2 py-1 text-xs/4 font-medium whitespace-normal text-fg-secondary ring-1 ring-quiet/20 ring-inset', className)} {...attributes}>
	{#if icon}<span class="shrink-0" aria-hidden="true">{@render icon()}</span>{/if}
	<span class="min-w-0 whitespace-nowrap truncate" title={label}>{label}</span>
</Badge>
