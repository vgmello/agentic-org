<script lang="ts">
	import { pageTitle } from '$lib/nav';
	import GeneralPanel from '$lib/modules/settings/features/general/GeneralPanel.svelte';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('General', 'Organization settings')}</title></svelte:head>

{#key data.connection.organization.id}<GeneralPanel organizationId={data.connection.organization.id} />{/key}
