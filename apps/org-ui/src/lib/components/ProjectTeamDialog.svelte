<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { toAppError } from '$lib/office/api';
	import { addProjectTeam } from '$lib/org/actions';
	import type { OrgSession } from '$lib/org/session';
	import { projectsToAdd, sharedBy, teamOptions } from '$lib/org/team-edit';
	import type { OrgSnapshot, Team } from '$lib/proto';

	/** Give a project to a team, from either side: pick a project for `team`, or a team for `project`. */
	let {
		open = $bindable(false),
		team,
		project,
		snapshot,
		session,
		ondone
	}: {
		open: boolean;
		/** From a team's menu: choose which project to give it. */
		team?: Team;
		/** From an unassigned project: choose which team gets it. */
		project?: { id: string; name: string };
		snapshot: OrgSnapshot;
		session: OrgSession;
		ondone: (result: { message?: string }) => void;
	} = $props();

	let choice = $state('');
	let error = $state<string | undefined>();
	let busy = $state(false);
	// Kept while the dialog closes, so its text doesn't flash empty.
	let shownTeam = $state<Team | undefined>();
	let shownProject = $state<{ id: string; name: string } | undefined>();

	$effect(() => {
		if (open) {
			untrack(() => {
				shownTeam = team;
				shownProject = project;
				choice = '';
				error = undefined;
			});
		}
	});

	const projects = $derived(shownTeam ? projectsToAdd(snapshot, shownTeam.id) : []);
	const teams = $derived(shownProject ? teamOptions(snapshot) : []);

	async function save() {
		if (!choice || busy) return;
		const projectId = shownTeam ? choice : shownProject?.id;
		const teamId = shownTeam ? shownTeam.id : choice;
		if (!projectId || !teamId) return;
		busy = true;
		try {
			await addProjectTeam(session, projectId, teamId);
			open = false;
			// "checkout is now shared by Checkout and Platform." (no confirmation was needed to add a team)
			const after = sharedBy(await session.snapshot(), projectId);
			const name = shownTeam ? (projects.find((p) => p.id === projectId)?.name ?? '') : (shownProject?.name ?? '');
			ondone({ message: after.length > 1 ? `${name} is now shared by ${after.slice(0, -1).join(', ')} and ${after.at(-1)}.` : `${name} now belongs to ${after[0] ?? 'the team'}.` });
		} catch (e) {
			error = toAppError(e).message;
			ondone({});
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = false)}>
	<DialogContent>
		<DialogHeader>
			{#if shownTeam}
				<DialogTitle class="[overflow-wrap:anywhere]">Add a project to {shownTeam.name}</DialogTitle>
				<DialogDescription>A project can have several teams: it shows on each of their cards as Shared.</DialogDescription>
			{:else}
				<DialogTitle class="[overflow-wrap:anywhere]">Assign a team to {shownProject?.name}</DialogTitle>
				<DialogDescription>The team's lead also leads the project.</DialogDescription>
			{/if}
		</DialogHeader>

		<fieldset class="max-h-64 space-y-1 overflow-y-auto" disabled={busy}>
			<legend class="sr-only">{shownTeam ? 'Project' : 'Team'}</legend>
			{#if shownTeam}
				{#each projects as p (p.id)}
					<label class="flex items-start gap-x-3 rounded-md px-2 py-1.5 text-sm/6 hover:bg-white/5">
						<input type="radio" name="project-team" value={p.id} class="mt-1" bind:group={choice} />
						<span class="min-w-0">
							<span class="block text-white [overflow-wrap:anywhere]">{p.name}</span>
							<span class="block text-xs/5 text-gray-400 [overflow-wrap:anywhere]">{p.teams.length ? `Teams: ${p.teams.join(', ')}` : 'No team'}</span>
						</span>
					</label>
				{:else}
					<p class="px-2 py-1.5 text-sm/6 text-gray-400">{shownTeam.name} already works on every project.</p>
				{/each}
			{:else}
				{#each teams as option (option.team.id)}
					<label class="flex items-start gap-x-3 rounded-md py-1.5 pr-2 text-sm/6 hover:bg-white/5" style="padding-left: {0.5 + option.depth * 1.25}rem">
						<input type="radio" name="project-team" value={option.team.id} class="mt-1" bind:group={choice} />
						<span class="min-w-0 text-white [overflow-wrap:anywhere]">{option.team.name}</span>
					</label>
				{:else}
					<p class="px-2 py-1.5 text-sm/6 text-gray-400">There are no teams yet. Create one first.</p>
				{/each}
			{/if}
		</fieldset>

		{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}

		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
			<Button type="button" disabled={busy || !choice} onclick={save}>{shownTeam ? 'Add project' : 'Assign team'}</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
