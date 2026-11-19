<script lang="ts">
	import HomeView from '$lib/modules/home/features/view/HomeView.svelte';
	import { pageTitle } from '$lib/nav';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Home')}</title></svelte:head>

<HomeView
	organizationId={data.connection.organization.id}
	organizationName={data.connection.organization.name}
/>
