<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { areaCapability } from '$lib/platform/workspace/capabilities';
	import { uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';
	import Unavailable from '$lib/platform/shell/Unavailable.svelte';
	import SessionInterruptions from '$lib/platform/auth/SessionInterruptions.svelte';

	let { children } = $props();
	const capability = $derived(areaCapability(page.url.pathname));
	onMount(() => {
		uiPrefs.hydrate();
		document.documentElement.dataset.hydrated = 'true';
	});
</script>

{#if capability.available || page.status === 404}
	{@render children()}
{:else}
	<Unavailable />
{/if}
<SessionInterruptions />
