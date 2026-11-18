<!--
  @component
  A team's Work tab (§7.2), the default: its tasks as divided rows, reference and title, project · assignee ·
  priority, and one trailing work-state pill. Rows don't repeat the team.
-->
<script lang="ts">
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { WorkRows } from '$lib/modules/members';
	import { teamsApi } from '../../api';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import { teamPageContext } from './context';

	const team = teamPageContext();

	const work = new Resource(() => teamsApi.work(team.organizationId, team.teamId));
</script>

<Loaded
	errorLevel={4}
	resource={work}
	label="the team's work"
	error="Couldn't load the team's work"
>
	{#snippet children(rows)}
		{#if rows.length === 0}
			<EmptyState
				level={4}
				title="No work yet"
				description="Tasks {team.teamName} is accountable for, or that are assigned to it, show here."
			/>
		{:else}
			<WorkRows {rows} />
		{/if}
	{/snippet}
</Loaded>
