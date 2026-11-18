<!--
  @component
  A team's Activity tab (§7.2): what its members did, newest first, each with actor, action and time.
-->
<script lang="ts">
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { ActivityRows } from '$lib/modules/members';
	import { teamsApi } from '../../api';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import { teamPageContext } from './context';

	const team = teamPageContext();

	const activity = new Resource(() => teamsApi.activity(team.organizationId, team.teamId));
</script>

<Loaded
	errorLevel={4}
	resource={activity}
	label="the team's activity"
	error="Couldn't load the team's activity"
>
	{#snippet children(rows)}
		{#if rows.length === 0}
			<EmptyState
				level={4}
				title="No activity yet"
				description="What {team.teamName}'s people and agents do shows here as it happens, newest first."
			/>
		{:else}
			<ActivityRows {rows} />
		{/if}
	{/snippet}
</Loaded>
