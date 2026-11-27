<script lang="ts">
	import { pageTitle } from '$lib/nav';
	import OfficesPage from '$lib/modules/offices/features/list/OfficesPage.svelte';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Offices', 'Organization settings')}</title></svelte:head>

<OfficesPage connection={data.connection} saved={data.offices} counts={data.counts} />
