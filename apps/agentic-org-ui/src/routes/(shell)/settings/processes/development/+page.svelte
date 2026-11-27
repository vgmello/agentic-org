<script lang="ts">
	import DevCatalogue from '$lib/modules/processes/features/dev-catalogue/DevCatalogue.svelte';

	let { data } = $props();
</script>

{#key data.connection.organization.id}<DevCatalogue
		organizationId={data.connection.organization.id}
	/>{/key}
