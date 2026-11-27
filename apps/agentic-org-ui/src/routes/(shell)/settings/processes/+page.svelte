<script lang="ts">
	import ProcessesPage from '$lib/modules/processes/features/place-view/ProcessesPage.svelte';

	let { data } = $props();
</script>

<!-- The page sets its own title from the place it shows (processes-ux PROC-32). -->
{#key data.connection.organization.id}<ProcessesPage
		organizationId={data.connection.organization.id}
	/>{/key}
