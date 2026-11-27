<script lang="ts">
	import { pageTitle } from '$lib/nav';
	import UpdatesPanel from '$lib/modules/updates/features/overview/UpdatesPanel.svelte';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Updates', 'Organization settings')}</title></svelte:head>

{#key data.connection.organization.id}<UpdatesPanel
		organizationId={data.connection.organization.id}
	/>{/key}
