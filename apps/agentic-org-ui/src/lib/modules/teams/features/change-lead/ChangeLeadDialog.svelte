<!--
  @component
  Change a team's lead (TEAM-15, MOVE-5): only employees whose home team is this team can lead it. The lead
  manages the whole subtree, including child-team leads; reporting lines do not change.
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { toAppError } from '$lib/office/api';
	import type { OrgSession } from '$lib/org/session';
	import type { LeadCandidate, TeamRow } from '../../types';

	let {
		open = $bindable(false),
		team,
		candidates,
		session,
		ondone
	}: {
		open: boolean;
		team: TeamRow | undefined;
		/** Active employees whose home team is this team; `undefined` while loading. */
		candidates: LeadCandidate[] | undefined;
		session: OrgSession;
		ondone: (result: { team: string; lead?: string }) => void;
	} = $props();

	let choice = $state('');
	let busy = $state(false);
	let error = $state<string | undefined>();
	let shown = $state<TeamRow | undefined>();
	$effect(() => {
		if (open)
			untrack(() => {
				shown = team;
				choice = team?.lead?.id ?? '';
				error = undefined;
			});
	});

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (!shown || busy) return;
		busy = true;
		try {
			const current = (await session.snapshot()).teams.find((t) => t.id === shown!.id);
			if (!current)
				throw { source: 'api', code: 'not_found', message: `${shown.name} isn't here any more.` };
			await session.command({
				method: 'team.set_lead',
				params: {
					team_id: shown.id,
					expected_version: current.version,
					lead_member_id: choice || undefined
				}
			});
			open = false;
			ondone({ team: shown.name, lead: candidates?.find((c) => c.id === choice)?.name });
		} catch (e) {
			error = toAppError(e).message;
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent>
		<form onsubmit={save} class="contents">
			<DialogHeader>
				<DialogTitle text={`Change the lead of ${shown?.name ?? ''}`} />
				<DialogDescription
					>The lead manages {shown?.name}'s whole subtree, including the leads of teams under it.
					Only someone whose home team is {shown?.name} can lead it. Reporting lines don't change.</DialogDescription
				>
			</DialogHeader>
			<fieldset class="space-y-1">
				<legend class="sr-only">Lead</legend>
				{#if candidates === undefined}
					<p class="text-sm/6 text-fg-muted" role="status">Loading who can lead…</p>
				{:else}
					<label
						class="flex min-h-11 items-center gap-x-3 rounded-md px-2 text-sm/6 text-fg hover:bg-surface-raised has-focus-visible:outline-2 has-focus-visible:outline-focus"
					>
						<input
							type="radio"
							name="lead"
							value=""
							bind:group={choice}
							class="size-4 accent-primary"
						/> No lead
					</label>
					{#each candidates as c (c.id)}
						<label
							class="flex min-h-11 items-center gap-x-3 rounded-md px-2 text-sm/6 text-fg hover:bg-surface-raised has-focus-visible:outline-2 has-focus-visible:outline-focus"
						>
							<input
								type="radio"
								name="lead"
								value={c.id}
								bind:group={choice}
								class="size-4 accent-primary"
							/>
							<span class="[overflow-wrap:anywhere]">{c.name}</span><span
								class="text-xs/5 text-fg-muted">{c.kind === 'agent' ? 'Agent' : 'Person'}</span
							>
						</label>
					{/each}
					{#if candidates.length === 0}
						<p class="text-sm/6 text-fg-muted">
							Nobody has {shown?.name} as their home team yet. Move someone here first with Move home
							team on their profile.
						</p>
					{/if}
				{/if}
			</fieldset>
			{#if error}<p class="text-sm/6 text-danger-text [overflow-wrap:anywhere]" role="alert">
					{error}
				</p>{/if}
			<DialogFooter>
				<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}
					>Cancel</Button
				>
				<Button type="submit" aria-busy={busy} disabled={busy || candidates === undefined}
					>{busy ? 'Saving…' : 'Save lead'}</Button
				>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
