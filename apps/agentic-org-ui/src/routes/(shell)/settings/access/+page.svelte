<script lang="ts">
	import { pageTitle } from '$lib/nav';
	import AccessPage from '$lib/modules/access/features/overview/AccessPage.svelte';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Members and access', 'Organization settings')}</title></svelte:head>

{#key data.connection.organization.id}<AccessPage
		organizationId={data.connection.organization.id}
		loads={data.loads}
	/>{/key}
