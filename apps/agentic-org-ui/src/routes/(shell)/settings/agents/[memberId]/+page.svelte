<script lang="ts">
	import { page } from '$app/state';
	import { AgentSetupPage } from '$lib/modules/agent-setup';

	let { data } = $props();
</script>

<!-- Another agent or organization is another page: its read starts again. The page sets its own title. -->
{#key `${data.connection.organization.id}/${page.params.memberId}`}<AgentSetupPage
		organizationId={data.connection.organization.id}
		memberId={page.params.memberId ?? ''}
		snapshot={data.snapshot}
	/>{/key}
