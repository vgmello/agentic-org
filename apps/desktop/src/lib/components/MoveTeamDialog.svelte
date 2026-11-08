<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { toAppError } from '$lib/office/api';
	import { moveTeam } from '$lib/org/actions';
	import type { OrgSession } from '$lib/org/session';
	import { moveChoices, movesAlong, teamPath } from '$lib/org/team-edit';
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
		/** Moved (with a way to undo it), or the tree changed at HQ meanwhile (`stale`): reload either way. */
		ondone: (result: { name: string; to: string; undo: () => Promise<void> } | { stale: true }) => void;
	} = $props();

	// '' is the top level.
	let target = $state<string | undefined>();
	let error = $state<string | undefined>();
	let busy = $state(false);
	// Kept while the dialog closes, so its text doesn't flash empty.
	let shown = $state<Team | undefined>();

	$effect(() => {
		if (open && team) {
			untrack(() => {
				shown = team;
				target = undefined;
				error = undefined;
			});
		}
	});

	const choices = $derived(shown ? moveChoices(snapshot, shown) : []);
	const atTop = $derived(!!shown && shown.owner_team_id === undefined);
	const from = $derived(shown?.owner_team_id ? teamPath(snapshot, shown.owner_team_id) : 'the top level');
	const to = $derived(target ? teamPath(snapshot, target) : 'the top level');

	async function confirm() {
		if (!shown || target === undefined || busy) return;
		busy = true;
		const name = shown.name;
		try {
			const { undo } = await moveTeam(session, shown.id, target || undefined);
			open = false;
			ondone({ name, to, undo });
		} catch (e) {
			error = `Couldn't move ${name}: ${toAppError(e).message} Nothing changed.`;
			ondone({ stale: true });
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent>
		<DialogHeader>
			<DialogTitle class="[overflow-wrap:anywhere]">Move {shown?.name} to…</DialogTitle>
			<DialogDescription>Pick the team it should be part of. Its members, projects and the teams under it stay with it.</DialogDescription>
		</DialogHeader>

		<fieldset class="max-h-64 space-y-1 overflow-y-auto" disabled={busy}>
			<legend class="sr-only">New owner team</legend>
			<label class="flex items-start gap-x-3 rounded-md px-2 py-1.5 text-sm/6 hover:bg-white/5 has-disabled:opacity-60">
				<input type="radio" name="move-target" value="" class="mt-1" checked={target === ''} disabled={atTop} onchange={() => (target = '')} />
				<span class="min-w-0"><span class="block text-white">None: top level</span>{#if atTop}<span class="block text-xs/5 text-gray-400">{shown?.name} is already at the top level</span>{/if}</span>
			</label>
			{#each choices as { option, disabled } (option.team.id)}
				<label class="flex items-start gap-x-3 rounded-md py-1.5 pr-2 text-sm/6 hover:bg-white/5 has-disabled:opacity-60" style="padding-left: {0.5 + option.depth * 1.25}rem">
					<input type="radio" name="move-target" value={option.team.id} class="mt-1" checked={target === option.team.id} disabled={!!disabled} onchange={() => (target = option.team.id)} />
					<span class="min-w-0">
						<span class="block text-white [overflow-wrap:anywhere]">{option.team.name}</span>
						{#if disabled}<span class="block text-xs/5 text-gray-400">{disabled}</span>{/if}
					</span>
				</label>
			{/each}
		</fieldset>

		{#if shown && target !== undefined}
			<p class="text-sm/6 text-gray-300 [overflow-wrap:anywhere]" data-testid="move-summary">
				Move {shown.name}{movesAlong(snapshot, shown)} from {from} to {to}? Members and projects stay with it.
			</p>
		{/if}
		{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}

		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
			<Button type="button" disabled={busy || target === undefined} onclick={confirm} class="[overflow-wrap:anywhere]">Move {shown?.name}</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
