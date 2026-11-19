<script lang="ts">
	import { pageTitle } from '$lib/nav';
	import { OrgChartPage } from '$lib/modules/org-chart';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Org chart', 'Teams')}</title></svelte:head>

<OrgChartPage connection={data.connection} snapshot={data.snapshot} loads={data.loads} />
