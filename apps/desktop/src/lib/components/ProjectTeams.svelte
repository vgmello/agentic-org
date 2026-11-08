<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import ProjectTeamDialog from '$lib/components/ProjectTeamDialog.svelte';
	import RemoveProjectDialog from '$lib/components/RemoveProjectDialog.svelte';
	import { toAppError } from '$lib/office/api';
	import { removeProjectTeam } from '$lib/org/actions';
	import type { ProjectRow } from '$lib/org/projects';
	import type { OrgSession } from '$lib/org/session';
	import { isLastTeam } from '$lib/org/team-edit';
	import type { OrgSnapshot } from '$lib/proto';

	/** A project's teams (TEAM-8): chips with × to take a team off, and Assign team… to add one. */
	let {
		row,
		snapshot,
		session,
		onchanged
	}: {
		row: ProjectRow;
		snapshot: OrgSnapshot;
		session: OrgSession;
		/** Something changed (or was refused): reload. */
		onchanged: () => void | Promise<void>;
	} = $props();

	let assignOpen = $state(false);
	let removeOpen = $state(false);
	let removing = $state<{ projectId: string; projectName: string; teamName: string; teamId: string } | undefined>();

	async function take(teamId: string, teamName: string) {
		// No confirmation while other teams remain; the last one asks first (it becomes unassigned).
		if (isLastTeam(snapshot, row.project.id, teamId)) {
			removing = { projectId: row.project.id, projectName: row.project.name, teamName, teamId };
			removeOpen = true;
			return;
		}
		try {
			await removeProjectTeam(session, row.project.id, teamId);
			toast.success(`Removed ${row.project.name} from ${teamName}.`);
		} catch (e) {
			toast.error(`Couldn't remove ${row.project.name} from ${teamName}: ${toAppError(e).message} Nothing changed.`);
		}
		await onchanged();
	}
	async function done(r: { message?: string }) {
		await onchanged();
		if (r.message) toast.success(r.message);
	}
</script>

<div class="flex flex-wrap items-center gap-x-2 gap-y-1.5">
	{#each row.teams as team (team.id)}
		<span class="inline-flex items-center gap-x-1 rounded-md bg-white/10 py-0.5 pr-0.5 pl-2 text-xs/5 text-gray-100" data-testid="project-team">
			{team.name}
			<button type="button" aria-label="Remove {row.project.name} from {team.name}" onclick={() => take(team.id, team.name)} class="rounded px-1 text-gray-400 hover:bg-white/10 hover:text-white focus-visible:outline-2 focus-visible:outline-indigo-400">×</button>
		</span>
	{/each}
	{#if row.shared}<Badge variant="secondary">Shared</Badge>{/if}
	{#if row.unassigned}<span class="text-xs/5 text-gray-400" data-testid="no-team">No team</span>{/if}
	<Button variant="outline" size="xs" aria-label="Assign a team to {row.project.name}" onclick={() => (assignOpen = true)}>{row.unassigned ? 'Assign team…' : 'Add team…'}</Button>
</div>

<ProjectTeamDialog bind:open={assignOpen} project={row.project} {snapshot} {session} ondone={done} />
<RemoveProjectDialog bind:open={removeOpen} target={removing} {session} ondone={done} />
