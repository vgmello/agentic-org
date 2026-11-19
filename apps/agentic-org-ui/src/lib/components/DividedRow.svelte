<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLLiAttributes } from 'svelte/elements';
	import { cn } from '$lib/utils';

	let { children, class: className, ...attributes }: HTMLLiAttributes & { children: Snippet } = $props();
</script>

<!-- A native list row, using the prototype's flat dividers and roomy spacing. -->
<li class={cn('relative min-w-0 border-b border-white/5 px-4 py-4 last:border-b-0 sm:px-6 lg:px-8', className)} {...attributes}>
	{@render children()}
</li>
