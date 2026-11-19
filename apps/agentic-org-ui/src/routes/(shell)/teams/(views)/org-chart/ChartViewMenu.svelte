<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent } from '$lib/components/ui/dropdown-menu';

	let { children }: { children: Snippet } = $props();
	// Open state dies with the menu, including when a live update removes its chart.
	let open = $state(false);
</script>

<DropdownMenu bind:open>
	<DropdownMenuTrigger>
		{#snippet child({ props })}
			<Button {...props} variant="outline" size="sm">View <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="m19.5 8.25-7.5 7.5-7.5-7.5" /></svg></Button>
		{/snippet}
	</DropdownMenuTrigger>
	<!-- Closed content remains mounted for its exit animation; reject stale focus into it.
	     preventScroll={false}: the body scroll lock also sets pointer-events:none on <body>, so a click on
	     another tab only closed the menu and needed a second click (BUG-159). -->
	<DropdownMenuContent inert={!open} preventScroll={false} align="end" class="w-48 max-w-[calc(100vw-2rem)]">
		{@render children()}
	</DropdownMenuContent>
</DropdownMenu>
