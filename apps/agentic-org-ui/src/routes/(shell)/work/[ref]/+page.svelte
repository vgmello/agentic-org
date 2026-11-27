<script lang="ts">
	import { page } from '$app/state';
	import WorkDetailPage from '$lib/modules/workitems/features/detail/WorkDetailPage.svelte';

	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const refParam = $derived(page.params.ref ?? '');
</script>

{#key `${organizationId}:${refParam}`}
	<WorkDetailPage {organizationId} {refParam} />
{/key}
