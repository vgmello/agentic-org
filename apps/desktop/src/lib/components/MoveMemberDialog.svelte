<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/office/api';
	import { moveMemberToTeam } from '$lib/org/actions';
	import { teamsOfMember } from '$lib/org/model';
	import type { OrgSession } from '$lib/org/session';
	import { moveMemberSummary, teamOptions } from '$lib/org/team-edit';
	import type { Member, OrgSnapshot } from '$lib/proto';

	let {
		open = $bindable(false),
		member,
		snapshot,
		session,
		ondone
	}: {
		open: boolean;
		member: Member | undefined;
		snapshot: OrgSnapshot;
		session: OrgSession;
		/** Moved (with a way to undo it), or HQ refused / things changed meanwhile (`stale`): reload either way. */
		ondone: (result: { name: string; to: string; undo: () => Promise<void> } | { stale: true }) => void;
	} = $props();

	let from = $state('');
	let target = $state<string | undefined>();
	let error = $state<string | undefined>();
	let busy = $state(false);
	let shown = $state<Member | undefined>();

	$effect(() => {
		if (open && member) {
			untrack(() => {
				shown = member;
				from = teamsOfMember(snapshot, member.id)[0]?.id ?? '';
				target = undefined;
				error = undefined;
			});
		}
	});

	const current = $derived(shown ? teamsOfMember(snapshot, shown.id) : []);
	const options = $derived(teamOptions(snapshot));
	const inTeam = $derived(new Set(current.map((t) => t.id)));
	const toName = $derived(snapshot.teams.find((t) => t.id === target)?.name ?? '');

	async function confirm() {
		if (!shown || !from || target === undefined || busy) return;
		busy = true;
		const name = shown.name;
		try {
			const { undo } = await moveMemberToTeam(session, shown.id, from, target);
			open = false;
			ondone({ name, to: toName, undo });
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
			<DialogTitle class="[overflow-wrap:anywhere]">Move {shown?.name} to a team</DialogTitle>
			<DialogDescription>One step: they leave one team and join the other.</DialogDescription>
		</DialogHeader>

		{#if current.length > 1}
			<div class="space-y-2">
				<Label for="move-from">From</Label>
				<select id="move-from" bind:value={from} disabled={busy} class="w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
					{#each current as t (t.id)}<option value={t.id}>{t.name}</option>{/each}
				</select>
			</div>
		{/if}

		<fieldset class="max-h-56 space-y-1 overflow-y-auto" disabled={busy}>
			<legend class="sr-only">Team to move to</legend>
			{#each options as option (option.team.id)}
				{@const taken = inTeam.has(option.team.id)}
				<label class="flex items-start gap-x-3 rounded-md py-1.5 pr-2 text-sm/6 hover:bg-white/5 has-disabled:opacity-60" style="padding-left: {0.5 + option.depth * 1.25}rem">
					<input type="radio" name="move-member-target" value={option.team.id} class="mt-1" checked={target === option.team.id} disabled={taken} onchange={() => (target = option.team.id)} />
					<span class="min-w-0">
						<span class="block text-white [overflow-wrap:anywhere]">{option.team.name}</span>
						{#if taken}<span class="block text-xs/5 text-gray-400">{shown?.name} is already in {option.team.name}</span>{/if}
					</span>
				</label>
			{/each}
		</fieldset>

		{#if shown && from && target !== undefined}
			<div class="space-y-1 text-sm/6 text-gray-300 [overflow-wrap:anywhere]" data-testid="move-member-summary">
				{#each moveMemberSummary(snapshot, shown, from, target) as line (line)}<p>{line}</p>{/each}
			</div>
		{/if}
		{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}

		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
			<Button type="button" disabled={busy || target === undefined} onclick={confirm} class="[overflow-wrap:anywhere]">Move {shown?.name}</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
