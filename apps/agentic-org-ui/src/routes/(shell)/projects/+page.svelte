<script lang="ts">
	import { pageTitle } from '$lib/nav';
	import { ProjectsList } from '$lib/modules/projects';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Projects')}</title></svelte:head>

{#key data.connection.organization.id}<ProjectsList
		organizationId={data.connection.organization.id}
	/>{/key}
