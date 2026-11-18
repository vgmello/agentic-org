<script lang="ts">
	import InboxView from '$lib/modules/inbox/features/view/InboxView.svelte';
	import { pageTitle } from '$lib/nav';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Inbox')}</title></svelte:head>

<InboxView organizationId={data.connection.organization.id} />
