<script lang="ts">
	// Hire requests (our-org-in-the-app §7.1): every hire with who hired, its home team and its state.
	import { pageTitle } from '$lib/nav';
	import { HireRequests } from '$lib/modules/employees';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Hire requests', 'Teams')}</title></svelte:head>

<HireRequests organizationId={data.connection.organization.id} />
