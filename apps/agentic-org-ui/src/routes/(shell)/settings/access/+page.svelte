<script lang="ts">
	import { AccessPage } from '$lib/modules/access';

	let { data } = $props();
</script>

{#key data.connection.organization.id}<AccessPage
		organizationId={data.connection.organization.id}
	/>{/key}
