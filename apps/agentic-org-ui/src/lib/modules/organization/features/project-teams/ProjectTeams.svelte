<script lang="ts">
	import { tick } from 'svelte';
	import { openerFrom } from '$lib/components/focus';
	import HeroIcon from '$lib/components/HeroIcon.svelte';
	import { toast } from 'svelte-sonner';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { ModalTooltip } from '$lib/components/ui/tooltip';
	import ProjectTeamDialog from '$lib/modules/organization/features/project-teams/ProjectTeamDialog.svelte';
	import RemoveProjectDialog from '$lib/modules/organization/features/project-teams/RemoveProjectDialog.svelte';
	import { toAppError } from '$lib/proto';
	import { removeProjectTeam } from '$lib/modules/organization/actions';
	import type { ProjectRow } from '$lib/modules/organization/projects';
	import type { OrgSession } from '$lib/modules/organization/session';
	import { isLastTeam } from '$lib/modules/organization/team-edit';
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
	let removing = $state<
		{ projectId: string; projectName: string; teamName: string; teamId: string } | undefined
	>();

	let chips = $state<HTMLDivElement>();
	let removeOpener = $state<HTMLElement | null>(null);
	let restoreLast: (() => Promise<void>) | undefined;
	function removalFocus(teamId: string) {
		const removed = chips?.querySelector(`[data-team="${CSS.escape(teamId)}"]`);
		const keys = row.teams.map((team) => team.id),
			at = keys.indexOf(teamId);
		const candidates = [...keys.slice(at + 1), ...keys.slice(0, at).reverse()];
		return async () => {
			await tick();
			if (document.activeElement !== document.body && !removed?.contains(document.activeElement))
				return;
			const next = candidates
				.map((id) =>
					chips?.querySelector<HTMLButtonElement>(`[data-team="${CSS.escape(id)}"] button`)
				)
				.find(Boolean);
			(next ?? chips?.querySelector<HTMLButtonElement>('[data-assign-team]'))?.focus();
		};
	}
	async function removed(r: { message?: string }) {
		const restore = restoreLast;
		await done(r);
		if (r.message) await restore?.();
	}
	async function take(teamId: string, teamName: string, event: MouseEvent) {
		const restore = removalFocus(teamId);
		// No confirmation while other teams remain; the last one asks first (it becomes unassigned).
		if (isLastTeam(snapshot, row.project.id, teamId)) {
			removeOpener = openerFrom(event);
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
			toast.error(
				`Couldn't remove ${row.project.name} from ${teamName}: ${toAppError(e).message} Nothing changed.`
			);
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
		<span
			class="inline-flex items-center gap-x-1 rounded-md bg-surface-overlay py-0.5 pr-0.5 pl-2 text-xs/5 text-fg-bright"
			data-testid="project-team"
			data-team={team.id}
		>
			{team.name}
			<ModalTooltip label="Remove {row.project.name} from {team.name}">
				<button
					type="button"
					aria-label="Remove {row.project.name} from {team.name}"
					onclick={(e) => take(team.id, team.name, e)}
					class="inline-flex size-6 shrink-0 items-center justify-center rounded text-fg-muted hover:bg-surface-overlay hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
					><HeroIcon variant="mini" name="x-mark" class="size-4!" /></button
				>
			</ModalTooltip>
		</span>
	{/each}
	{#if row.shared}<Badge variant="secondary">Shared</Badge>{/if}
	{#if row.unassigned}<span class="text-xs/5 text-fg-muted" data-testid="no-team">No team</span
		>{/if}
	<Button
		data-assign-team
		variant="outline"
		size="xs"
		aria-label="Assign a team to {row.project.name}"
		onclick={() => (assignOpen = true)}>{row.unassigned ? 'Assign team…' : 'Add team…'}</Button
	>
</div>

<ProjectTeamDialog
	bind:open={assignOpen}
	project={row.project}
	{snapshot}
	{session}
	ondone={done}
/>
<RemoveProjectDialog
	bind:open={removeOpen}
	target={removing}
	opener={removeOpener}
	fallback={() => chips?.querySelector<HTMLButtonElement>('[data-assign-team]') ?? null}
	{session}
	ondone={removed}
/>
