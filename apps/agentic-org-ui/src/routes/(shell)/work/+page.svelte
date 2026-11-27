<script lang="ts">
	import { pageTitle } from '$lib/nav';
	import WorkQueueView from '$lib/modules/workitems/features/queue/WorkQueueView.svelte';

	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
</script>

<svelte:head><title>{pageTitle('Work')}</title></svelte:head>

{#key organizationId}
	<WorkQueueView {organizationId} scope={{ kind: 'organization' }} />
{/key}
