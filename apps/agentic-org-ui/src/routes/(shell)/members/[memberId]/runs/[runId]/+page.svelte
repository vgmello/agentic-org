<script lang="ts">
	import { page } from '$app/state';
	import SessionLogView from '$lib/modules/members/features/session-log/SessionLogView.svelte';

	let { data } = $props();
</script>

{#key `${page.params.memberId}/${page.params.runId}`}
	<SessionLogView
		organizationId={data.connection.organization.id}
		memberId={page.params.memberId ?? ''}
		memberName={data.memberName}
		runId={page.params.runId ?? ''}
	/>
{/key}
