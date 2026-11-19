<!--
  @component
  A team's Processes tab (processes-ux §2 "Where"): the organization processes and standards that apply to the team,
  each with the level it comes from, its revision and whether it replaces one above, is added here or is not
  overridable. Read and shown by the processes module; writing and overriding happen in Organization settings ›
  Processes.
-->
<script lang="ts">
	import { Resource } from '$lib/components/resource.svelte';
	import { PlaceProcesses, readPlace } from '$lib/modules/processes';
	import { teamPageContext } from './context';
	import { reloadOnChange } from './reload.svelte';

	const team = teamPageContext();
	const place = { kind: 'team', id: team.teamId } as const;

	const processes = new Resource(() => readPlace(team.organizationId, place));
	reloadOnChange(processes, () => team.loads);
</script>

<PlaceProcesses resource={processes} {place} name={team.teamName} headingLevel={3} />
