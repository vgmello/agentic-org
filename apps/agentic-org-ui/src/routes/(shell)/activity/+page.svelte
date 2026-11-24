<script lang="ts">
	import ActivityCommandCenter from '$lib/modules/activity/features/command-center/ActivityCommandCenter.svelte';
	import { pageTitle } from '$lib/nav';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Activity')}</title></svelte:head>

<ActivityCommandCenter
	organizationId={data.connection.organization.id}
	organizationName={data.connection.organization.name}
	snapshot={data.snapshot}
/>
