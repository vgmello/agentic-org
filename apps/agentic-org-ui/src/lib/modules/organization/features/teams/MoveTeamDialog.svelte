<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { toAppError } from '$lib/proto';
	import { moveTeam } from '$lib/modules/organization/actions';
	import type { OrgSession } from '$lib/modules/organization/session';
	import { moveChoices, movesAlong, teamPath } from '$lib/modules/organization/team-edit';
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
		ondone: (
			result: { name: string; to: string; undo: () => Promise<void> } | { stale: true }
		) => void;
	} = $props();

	// '' is the top level.
	let target = $state<string | undefined>();
	let error = $state<string | undefined>();
	let busy = $state(false);
	// Kept while the dialog closes, so its text doesn't flash empty.
	let lastTeam = $state<Team | undefined>();
	const shown = $derived(open ? (team ?? lastTeam) : lastTeam);

	function resetOnOpen() {
		// Only opening re-initialises the form: a reload behind the dialog (a stale edit) must keep what was typed.
		if (team) lastTeam = team;
		target = undefined;
		error = undefined;
	}

	const choices = $derived(shown ? moveChoices(snapshot, shown) : []);
	const atTop = $derived(!!shown && shown.owner_team_id === undefined);
	const from = $derived(
		shown?.owner_team_id ? teamPath(snapshot, shown.owner_team_id) : 'the top level'
	);
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
	<DialogContent onOpenAutoFocus={resetOnOpen}>
		<DialogHeader>
			<DialogTitle text={`Move ${shown?.name ?? ''} to…`} />
			<DialogDescription
				>Pick the team it should be part of. Its members, projects and the teams under it stay with
				it.</DialogDescription
			>
		</DialogHeader>

		<fieldset class="max-h-64 space-y-1 overflow-y-auto" disabled={busy}>
			<legend class="sr-only">New owner team</legend>
			<label
				class="flex items-start gap-x-3 rounded-md px-2 py-1.5 text-sm/6 hover:bg-surface-raised has-disabled:opacity-60"
			>
				<input
					type="radio"
					name="move-target"
					value=""
					class="mt-1"
					checked={target === ''}
					disabled={atTop}
					onchange={() => (target = '')}
				/>
				<span class="min-w-0"
					><span class="block text-fg">None: top level</span>{#if atTop}<span
							class="block text-xs/5 text-fg-muted">{shown?.name} is already at the top level</span
						>{/if}</span
				>
			</label>
			{#each choices as { option, disabled } (option.team.id)}
				<label
					class="flex items-start gap-x-3 rounded-md py-1.5 pr-2 text-sm/6 hover:bg-surface-raised has-disabled:opacity-60"
					style="padding-left: {0.5 + option.depth * 1.25}rem"
				>
					<input
						type="radio"
						name="move-target"
						value={option.team.id}
						class="mt-1"
						checked={target === option.team.id}
						disabled={!!disabled}
						onchange={() => (target = option.team.id)}
					/>
					<span class="min-w-0">
						<span class="block text-fg [overflow-wrap:anywhere]">{option.team.name}</span>
						{#if disabled}<span class="block text-xs/5 text-fg-muted">{disabled}</span>{/if}
					</span>
				</label>
			{/each}
		</fieldset>

		{#if shown && target !== undefined}
			<p class="text-sm/6 text-fg-secondary [overflow-wrap:anywhere]" data-testid="move-summary">
				Move {shown.name}{movesAlong(snapshot, shown)} from {from} to {to}? Members and projects
				stay with it.
			</p>
		{/if}
		{#if error}<p class="text-sm/6 text-danger [overflow-wrap:anywhere]" role="alert">
				{error}
			</p>{/if}

		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}
				>Cancel</Button
			>
			<Button
				type="button"
				disabled={busy || target === undefined}
				onclick={confirm}
				class="[overflow-wrap:anywhere]">Move {shown?.name}</Button
			>
		</DialogFooter>
	</DialogContent>
</Dialog>
