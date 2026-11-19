<!--
  @component
  A team's Work tab (§7.2), the default: its tasks as the members module's work rows (reference and title, project ·
  assignee · priority, one trailing work-state pill), so the words and colours stay one copy. Rows don't repeat the
  team. Rows don't link yet: the work item page can't open a task, and a link to a page that says it isn't built is a
  functioning-looking no-op (§6a); `linked={false}` says so. Once the work item page exists, drop it.
-->
<script lang="ts">
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { WorkRows } from '$lib/modules/members';
	import { teamsApi } from '../../api';
	import { teamPageContext } from './context';
	import { reloadOnChange } from './reload.svelte';

	const team = teamPageContext();

	const work = new Resource(() => teamsApi.work(team.organizationId, team.teamId));
	reloadOnChange(work, () => team.loads);
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
			<WorkRows {rows} linked={false} />
		{/if}
	{/snippet}
</Loaded>
