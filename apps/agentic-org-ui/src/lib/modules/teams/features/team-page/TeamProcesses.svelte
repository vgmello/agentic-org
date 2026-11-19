<!--
  @component
  A team's Processes tab (processes-ux §2 "Where"): the effective set for the team, each with the level it comes
  from, its revision and whether it is inherited, replaces the organization's, is added here or is not
  overridable. Writing and overriding processes happens in Organization settings › Processes.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { ProcessRows } from '$lib/modules/members';
	import { teamsApi } from '../../api';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import { teamPageContext } from './context';
	import { reloadOnChange } from './reload.svelte';

	const team = teamPageContext();

	const processes = new Resource(() => teamsApi.processes(team.organizationId, team.teamId));
	reloadOnChange(processes, () => team.loads);
</script>

<Loaded
	errorLevel={4}
	resource={processes}
	label="the team's processes"
	error="Couldn't load the team's processes"
>
	{#snippet children(rows)}
		{#if rows.length === 0}
			<EmptyState
				level={4}
				title="No processes yet"
				description="Write down how your agents work. Start with how changes get reviewed and merged."
			>
				{#snippet action()}
					<Button variant="outline" href={resolve('/settings/processes')} class="max-[639px]:h-11"
						>Open Processes</Button
					>
				{/snippet}
			</EmptyState>
		{:else}
			<ProcessRows {rows} />
		{/if}
	{/snippet}
</Loaded>
