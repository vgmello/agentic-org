<script lang="ts">
	import SearchPage from '$lib/modules/search/features/results/SearchPage.svelte';

	let { data } = $props();
</script>

<SearchPage organizationId={data.connection.organization.id} />
