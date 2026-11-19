<script lang="ts">
	import { pageTitle } from '$lib/nav';
	import { AgentSetupList } from '$lib/modules/agent-setup';
	import { hireUrl } from '$lib/modules/employees';

	let { data } = $props();
</script>

<svelte:head><title>{pageTitle('Agent setup', 'Organization settings')}</title></svelte:head>

<!-- The route composes the two modules: Agent setup's Hire an agent opens the employees module's hire flow. -->
{#key data.connection.organization.id}<AgentSetupList
		organizationId={data.connection.organization.id}
		hireHref={hireUrl()}
	/>{/key}
