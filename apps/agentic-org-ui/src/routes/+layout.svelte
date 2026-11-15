<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { areaCapability } from '$lib/platform/shell/capabilities';
	import Unavailable from '$lib/platform/shell/Unavailable.svelte';

	let { children } = $props();
	const capability = $derived(areaCapability(page.url.pathname));
	onMount(() => {
		document.documentElement.dataset.hydrated = 'true';
	});
</script>

{#if capability.available || page.status === 404}
	{@render children()}
{:else}
	<Unavailable />
{/if}
