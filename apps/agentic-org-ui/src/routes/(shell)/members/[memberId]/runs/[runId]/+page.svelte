<script lang="ts">
	import { page } from '$app/state';
	import { SessionLogView } from '$lib/modules/members';

	let { data } = $props();
</script>

{#key `${page.params.memberId}/${page.params.runId}`}
	<SessionLogView
		organizationId={data.connection.organization.id}
		memberId={page.params.memberId ?? ''}
		runId={page.params.runId ?? ''}
	/>
{/key}
