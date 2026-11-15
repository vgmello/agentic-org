<script lang="ts">
	import { page } from '$app/state';
	import { Badge } from '$lib/components/ui/badge';
	import { navFor } from '$lib/nav';

	const item = $derived(navFor(page.url.pathname));
</script>

<svelte:head><title>{item?.name} · Agentic Org</title></svelte:head>

<div class="flex flex-col items-center px-8 py-24 text-center">
	<h2 class="text-base/7 font-semibold text-white">{item?.name}</h2>
	<p class="mt-1 max-w-sm text-sm/6 text-gray-400">This part of Agentic Org isn't built yet.</p>
	<Badge variant="secondary" class="mt-4">Coming soon</Badge>
</div>
