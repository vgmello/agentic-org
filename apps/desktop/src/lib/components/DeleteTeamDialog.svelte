<script lang="ts">
	import { untrack } from 'svelte';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { toAppError } from '$lib/office/api';
	import type { OrgSession } from '$lib/org/session';
	import { deleteTeam } from '$lib/org/actions';
	import { deleteSummary } from '$lib/org/team-edit';
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
		ondone: (result: { name: string } | { stale: true }) => void;
	} = $props();

	let busy = $state(false);
	let error = $state<string | undefined>();
	// Kept while the dialog closes, so its text doesn't flash empty.
	let shownTeam = $state<Team | undefined>();
	$effect(() => {
		// Only opening re-initialises the form: a reload behind the dialog (a stale edit) must keep what was typed.
		if (open) {
			untrack(() => {
				if (!(team)) return;
				shownTeam = team;
				error = undefined;
			});
		}
	});
	const summary = $derived(shownTeam ? deleteSummary(snapshot, shownTeam) : { refused: undefined, lines: [] });

	async function confirm() {
		if (!team || busy) return;
		busy = true;
		try {
			await deleteTeam(session, team);
			open = false;
			ondone({ name: team.name });
		} catch (e) {
			const failure = toAppError(e);
			error = failure.code === 'conflict' ? `${team.name} changed at HQ while you were looking. Here's the latest.` : failure.message;
			if (failure.code === 'conflict' || failure.code === 'not_found') ondone({ stale: true });
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent>
		<DialogHeader>
			<div class="mb-2 flex size-12 items-center justify-center rounded-full bg-red-500/15 text-red-400"><Trash2 class="size-6" aria-hidden="true" /></div>
			<DialogTitle class="[overflow-wrap:anywhere]">{summary.refused ? `Can't delete ${shownTeam?.name}` : `Delete ${shownTeam?.name}?`}</DialogTitle>
			<DialogDescription class="space-y-1 [overflow-wrap:anywhere]">
				{#if summary.refused}
					<span class="block">{summary.refused}</span>
				{:else}
					{#each summary.lines as line (line)}<span class="block">{line}</span>{/each}
				{/if}
			</DialogDescription>
			{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
		</DialogHeader>
		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>{summary.refused ? 'Close' : 'Cancel'}</Button>
			{#if !summary.refused}
				<Button type="button" variant="destructive" disabled={busy} onclick={confirm} class="[overflow-wrap:anywhere]">Delete {shownTeam?.name}</Button>
			{/if}
		</DialogFooter>
	</DialogContent>
</Dialog>
