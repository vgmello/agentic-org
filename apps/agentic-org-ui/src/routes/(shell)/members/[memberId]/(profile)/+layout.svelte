<script lang="ts">
	import { page } from '$app/state';
	import ProfileFrame from '$lib/modules/members/features/profile/ProfileFrame.svelte';

	let { data, children } = $props();
</script>

<!-- One frame per member: moving to another member's profile starts its reads afresh. -->
{#key page.params.memberId}
	<ProfileFrame
		organizationId={data.connection.organization.id}
		organizationName={data.connection.organization.name}
		memberId={page.params.memberId ?? ''}
		snapshot={data.snapshot}
	>
		{@render children()}
	</ProfileFrame>
{/key}
