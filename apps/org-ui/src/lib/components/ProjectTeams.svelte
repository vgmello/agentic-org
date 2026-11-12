<script lang="ts">
	import { tick } from 'svelte';
	import HeroIcon from './HeroIcon.svelte';
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

	let chips = $state<HTMLDivElement>();
	let restoreLast: (() => Promise<void>) | undefined;
	function removalFocus(teamId: string) {
		const removed = chips?.querySelector(`[data-team="${CSS.escape(teamId)}"]`);
		const keys = row.teams.map((team) => team.id), at = keys.indexOf(teamId);
		const candidates = [...keys.slice(at + 1), ...keys.slice(0, at).reverse()];
		return async () => {
			await tick();
			if (document.activeElement !== document.body && !removed?.contains(document.activeElement)) return;
			const next = candidates.map((id) => chips?.querySelector<HTMLButtonElement>(`[data-team="${CSS.escape(id)}"] button`)).find(Boolean);
			(next ?? chips?.querySelector<HTMLButtonElement>('[data-assign-team]'))?.focus();
		};
	}
	async function removed(r: { message?: string }) {
		await done(r);
		if (r.message) await restoreLast?.();
	}
	async function take(teamId: string, teamName: string) {
		const restore = removalFocus(teamId);
		// No confirmation while other teams remain; the last one asks first (it becomes unassigned).
		if (isLastTeam(snapshot, row.project.id, teamId)) {
			removing = { projectId: row.project.id, projectName: row.project.name, teamName, teamId };
			restoreLast = restore;
			removeOpen = true;
			return;
		}
		let changed = false;
		try {
			await removeProjectTeam(session, row.project.id, teamId);
			toast.success(`Removed ${row.project.name} from ${teamName}.`);
			changed = true;
		} catch (e) {
			toast.error(`Couldn't remove ${row.project.name} from ${teamName}: ${toAppError(e).message} Nothing changed.`);
		}
		await onchanged();
		if (changed) await restore();
	}
	async function done(r: { message?: string }) {
		await onchanged();
		if (r.message) toast.success(r.message);
	}
</script>

<div bind:this={chips} class="flex flex-wrap items-center gap-x-2 gap-y-1.5">
	{#each row.teams as team (team.id)}
		<span class="inline-flex items-center gap-x-1 rounded-md bg-white/10 py-0.5 pr-0.5 pl-2 text-xs/5 text-gray-100" data-testid="project-team" data-team={team.id}>
			{team.name}
			<button type="button" aria-label="Remove {row.project.name} from {team.name}"
					title="Remove {row.project.name} from {team.name}" onclick={() => take(team.id, team.name)} class="inline-flex size-6 shrink-0 items-center justify-center rounded text-gray-400 hover:bg-white/10 hover:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"><HeroIcon variant="mini" name="x-mark" class="size-4!" /></button>
		</span>
	{/each}
	{#if row.shared}<Badge variant="secondary">Shared</Badge>{/if}
	{#if row.unassigned}<span class="text-xs/5 text-gray-400" data-testid="no-team">No team</span>{/if}
	<Button data-assign-team variant="outline" size="xs" aria-label="Assign a team to {row.project.name}" onclick={() => (assignOpen = true)}>{row.unassigned ? 'Assign team…' : 'Add team…'}</Button>
</div>

<ProjectTeamDialog bind:open={assignOpen} project={row.project} {snapshot} {session} ondone={done} />
<RemoveProjectDialog bind:open={removeOpen} target={removing} {session} ondone={removed} />
