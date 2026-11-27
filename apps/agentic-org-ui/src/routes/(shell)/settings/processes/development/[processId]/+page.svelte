<script lang="ts">
	import { page } from '$app/state';
	import DevProcessPage from '$lib/modules/processes/features/dev-process/DevProcessPage.svelte';

	let { data } = $props();
</script>

<!-- Another development process or organization is another page. The page sets its own title. -->
{#key `${data.connection.organization.id}/${page.params.processId}`}<DevProcessPage
		organizationId={data.connection.organization.id}
		processId={page.params.processId ?? ''}
	/>{/key}
