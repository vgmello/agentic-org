<script lang="ts">
	import { pageTitle } from '$lib/nav';
	import TeamsIndex from '$lib/modules/teams/features/browse-teams/TeamsIndex.svelte';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Teams')}</title></svelte:head>

<TeamsIndex organizationId={data.connection.organization.id} snapshot={data.snapshot} />
