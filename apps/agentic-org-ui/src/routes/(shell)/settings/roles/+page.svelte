<script lang="ts">
	import { pageTitle } from '$lib/nav';
	import RolesList from '$lib/modules/roles/features/list/RolesList.svelte';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Roles', 'Organization settings')}</title></svelte:head>

{#key data.connection.organization.id}<RolesList
		organizationId={data.connection.organization.id}
	/>{/key}
