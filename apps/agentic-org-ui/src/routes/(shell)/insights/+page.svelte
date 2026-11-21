<script lang="ts">
	// Organization Insights `/insights`: thin route composing OrganizationInsightsView with the viewer's time zone.
	import { pageTitle } from '$lib/nav';
	import { OrganizationInsightsView } from '$lib/modules/costs';
	import { accountZone } from '$lib/modules/preferences';

	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
	$effect(() => void accountZone.ensure(organizationId));
</script>

<svelte:head><title>{pageTitle('Insights')}</title></svelte:head>

{#if accountZone.loaded}
	{#key `${organizationId}|${accountZone.zone}`}
		<OrganizationInsightsView {organizationId} zone={accountZone.zone} />
	{/key}
{/if}
