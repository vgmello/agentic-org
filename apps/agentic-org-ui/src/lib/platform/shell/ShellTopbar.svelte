<script lang="ts">
	import type { Snippet } from 'svelte';

	let { heading, children }: { heading?: string; children: Snippet } = $props();
</script>

<div data-shell-header class="sticky top-0 z-40 flex h-14 shrink-0 items-center gap-x-6 border-b border-white/5 bg-gray-800 px-4 min-[640px]:h-16 min-[640px]:px-6 min-[1024px]:px-8">
	{@render children()}
	<h1 id="shell-page-title" tabindex="-1" class="min-w-0 text-base/7 font-semibold text-white focus:outline-none">{heading}</h1>
</div>
