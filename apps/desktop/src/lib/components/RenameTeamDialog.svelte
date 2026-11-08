<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/office/api';
	import type { OrgSession } from '$lib/org/session';
	import { checkTeamName } from '$lib/org/team-edit';
	import type { OrgSnapshot, Team } from '$lib/proto';

	let {
		open = $bindable(false),
		team,
		snapshot,
		session,
		ondone
	}: {
		open: boolean;
		team: Team | undefined;
		snapshot: OrgSnapshot;
		session: OrgSession;
		/** Renamed, or the team changed at HQ meanwhile (`stale`): reload either way. */
		ondone: (result: { name?: string; stale?: boolean }) => void;
	} = $props();

	let name = $state('');
	let error = $state<string | undefined>();
	let busy = $state(false);
	// Kept while the dialog closes, so its title doesn't flash empty.
	let title = $state('');

	$effect(() => {
		// Only opening re-initialises the form: a reload behind the dialog (a stale edit) must keep what was typed.
		if (open) {
			untrack(() => {
				if (!(team)) return;
				untrack(() => {
					name = team.name;
					title = team.name;
					error = undefined;
				});
			});
		}
	});

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		if (!team || busy) return;
		const checked = checkTeamName(snapshot, name, team.owner_team_id, team.id);
		if (!checked.ok) {
			error = checked.message;
			return;
		}
		if (checked.name === team.name) {
			open = false;
			return;
		}
		busy = true;
		try {
			await session.command({ method: 'team.rename', params: { team_id: team.id, expected_version: team.version, name: checked.name } });
			open = false;
			ondone({ name: checked.name });
		} catch (e) {
			const failure = toAppError(e);
			if (failure.code === 'conflict') {
				// Stale edit (spec §7.10): show the latest, keep what was typed.
				error = `${title} changed at HQ while you were editing. Here's the latest.`;
				ondone({ stale: true });
			} else error = failure.message;
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent>
		<form class="grid gap-6" onsubmit={submit} novalidate>
			<DialogHeader>
				<DialogTitle class="[overflow-wrap:anywhere]">Rename {title}</DialogTitle>
				<DialogDescription>The team keeps its members, projects and the teams under it.</DialogDescription>
			</DialogHeader>
			<div class="space-y-2">
				<Label for="rename-team">Name</Label>
				<Input id="rename-team" bind:value={name} disabled={busy} autocomplete="off" aria-invalid={error ? true : undefined} aria-describedby={error ? 'rename-team-error' : undefined} oninput={() => (error = undefined)} />
				{#if error}<p id="rename-team-error" class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
			</div>
			<DialogFooter>
				<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={busy}>Rename</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
