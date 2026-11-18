<script lang="ts">
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { ActivityFeed } from '$lib/modules/activity';
	import { pageTitle } from '$lib/nav';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Activity')}</title></svelte:head>

<PageHeader
	title="Activity"
	description="What your teams and agents are doing, newest first. Decisions show under the Decisions kind."
/>
<ActivityFeed organizationId={data.connection.organization.id} syncUrl />
