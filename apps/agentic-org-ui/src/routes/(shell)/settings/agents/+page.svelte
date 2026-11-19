<script lang="ts">
	import { pageTitle } from '$lib/nav';
	import { AgentSetupList } from '$lib/modules/agent-setup';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Agent setup', 'Organization settings')}</title></svelte:head>

{#key data.connection.organization.id}<AgentSetupList
		organizationId={data.connection.organization.id}
	/>{/key}
