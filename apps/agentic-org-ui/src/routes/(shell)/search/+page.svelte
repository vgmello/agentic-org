<script lang="ts">
	import { SearchPage } from '$lib/modules/search';

	let { data } = $props();
</script>

<SearchPage organizationId={data.connection.organization.id} />
