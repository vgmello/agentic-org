<script lang="ts">
	import { DevCatalogue } from '$lib/modules/processes';

	let { data } = $props();
</script>

{#key data.connection.organization.id}<DevCatalogue
		organizationId={data.connection.organization.id}
	/>{/key}
