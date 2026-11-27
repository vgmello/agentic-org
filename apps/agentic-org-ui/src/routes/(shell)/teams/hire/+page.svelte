<script lang="ts">
	// Hire an agent (our-org-in-the-app §7): beside the Teams view switch, not one of its views.
	import { afterNavigate } from '$app/navigation';
	import { page } from '$app/state';
	import { pageTitle } from '$lib/nav';
	import HireFlow from '$lib/modules/employees/features/hire/HireFlow.svelte';

	let { data } = $props();

	// Opening Hire again while it is open (New › Hire an agent) starts a fresh flow. The flow's own address updates
	// are shallow and don't count as navigation.
	let opened = $state(0);
	afterNavigate((move) => {
		if (move.type !== 'enter' && move.from?.url?.pathname === move.to?.url?.pathname) opened++;
	});
</script>

<svelte:head><title>{pageTitle('Hire an agent', 'Teams')}</title></svelte:head>

{#key opened}
	<HireFlow
		organizationId={data.connection.organization.id}
		organizationName={data.connection.organization.name}
		teamId={page.url.searchParams.get('team') ?? undefined}
		requestId={page.url.searchParams.get('request') ?? undefined}
	/>
{/key}
