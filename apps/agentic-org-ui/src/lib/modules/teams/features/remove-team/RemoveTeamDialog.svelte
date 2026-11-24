<!--
  @component
  Archive or delete a team (offices-and-teams-ux §7.10, TEAM-9). Archive keeps its ID and history and is the one to
  prefer; Delete removes it, with no cascade. Before either is offered the caller checks what still points at the team
  (`check`): while teams under it, home employees, projects it owns or open tasks remain, the dialog says it can't,
  lists each blocker with a link to where it is resolved, and offers no confirm button. HQ still decides: a refusal
  after the check (something changed meanwhile) is shown in the dialog and the team is kept.
-->
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
	import { teamsApi } from '../../api';
	import { blockedSentence, isBlocked, type BlockerCheck, type TeamRemoval } from '../../blockers';
	import TeamBlockers from '../../components/TeamBlockers.svelte';
	import type { TeamRow } from '../../types';

	let {
		open = $bindable(false),
		removal,
		team,
		organizationId,
		check,
		effects = [],
		onrecheck,
		ondone
	}: {
		open: boolean;
		removal: TeamRemoval;
		team: TeamRow | undefined;
		organizationId: string;
		/** What still points at the team, checked when the dialog opens. */
		check: BlockerCheck;
		/** Delete: what else changes once nothing blocks it ("Beacon stays with Design."). */
		effects?: string[];
		/** Runs the check again after it failed. */
		onrecheck: () => void;
		ondone: (result: { name: string }) => void;
	} = $props();

	let busy = $state(false);
	let error = $state<string | undefined>();
	const blockers = $derived(
		check.state === 'done' && isBlocked(check.blockers) ? check.blockers : undefined
	);
	const workCheckError = $derived(check.state === 'done' ? check.workCheckError : undefined);
	// The caller keeps `team` after closing, so the text doesn't flash empty while the dialog leaves.
	const name = $derived(team?.name ?? '');
	const verb = $derived(removal === 'archive' ? 'Archive' : 'Delete');

	/** Closing forgets a refusal, so the next opening starts clean. */
	function close() {
		if (busy) return;
		open = false;
		error = undefined;
	}

	async function confirm() {
		if (!team || busy || blockers || check.state === 'checking') return;
		busy = true;
		try {
			await (removal === 'archive'
				? teamsApi.archive(organizationId, team.id)
				: teamsApi.delete(organizationId, team.id));
			open = false;
			ondone({ name: team.name });
		} catch (e) {
			error = toAppError(e).message;
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && (busy ? (open = true) : (error = undefined))}>
	<DialogContent>
		<DialogHeader>
			<DialogTitle text={blockers ? `Can't ${verb.toLowerCase()} ${name}` : `${verb} ${name}?`} />
			<DialogDescription class="space-y-2 [overflow-wrap:anywhere]">
				{#if blockers}
					<span class="block">{blockedSentence(name, blockers)}</span>
					<span class="block"
						>Move or archive each team under it, give each employee another home team from their
						profile, and give its projects and open tasks another owner team.</span
					>
				{:else if removal === 'archive'}
					<span class="block"
						>Its ID, reference and history stay. Archived teams take no new work and are listed
						apart.</span
					>
					<span class="block"
						>A team can be archived once nothing it owns or holds is left: no teams under it, no
						home employees, no projects it owns and no open tasks.</span
					>
				{:else}
					<span class="block"
						>{name} is removed for good. Archive keeps its ID and history instead.</span
					>
					{#each effects as line (line)}<span class="block">{line}</span>{/each}
				{/if}
			</DialogDescription>
		</DialogHeader>
		{#if blockers}
			<TeamBlockers {name} {blockers} onleave={close} />
		{:else if check.state === 'checking'}
			<p id="remove-team-check" role="status" class="text-sm/6 text-fg-muted">
				Checking what's still in {name}…
			</p>
		{:else if check.state === 'failed'}
			<div id="remove-team-check" role="alert" class="text-sm/6">
				<p class="text-danger-text [overflow-wrap:anywhere]">
					Couldn't check what's still in {name}: {check.message}
				</p>
				<p class="mt-1 text-fg-muted">
					Check again, or go ahead: HQ refuses while anything still points at {name}.
				</p>
			</div>
		{/if}
		{#if workCheckError}
			<div id="remove-team-work-check" role="alert" class="text-sm/6">
				<p class="text-danger-text [overflow-wrap:anywhere]">
					Couldn't check open tasks for {name}: {workCheckError}
				</p>
				<p class="mt-1 text-fg-muted">
					Check again, or go ahead: HQ refuses while anything still points at {name}.
				</p>
			</div>
		{/if}
		{#if error}<p class="text-sm/6 text-danger-text [overflow-wrap:anywhere]" role="alert">
				{error}
			</p>{/if}
		<DialogFooter>
			{#if blockers}
				{#if workCheckError}
					<Button type="button" variant="outline" disabled={busy} onclick={onrecheck}
						>Check again</Button
					>
				{/if}
				<Button type="button" variant="outline" onclick={close}>Close</Button>
			{:else}
				<Button type="button" variant="outline" disabled={busy} onclick={close}>Cancel</Button>
				{#if check.state === 'failed' || workCheckError}
					<Button type="button" variant="outline" disabled={busy} onclick={onrecheck}
						>Check again</Button
					>
				{/if}
				<Button
					type="button"
					variant="destructive"
					aria-busy={busy}
					aria-describedby={check.state === 'checking' ? 'remove-team-check' : undefined}
					disabled={busy || check.state === 'checking'}
					class="[overflow-wrap:anywhere]"
					onclick={confirm}
					>{busy ? (removal === 'archive' ? 'Archiving…' : 'Deleting…') : `${verb} ${name}`}</Button
				>
			{/if}
		</DialogFooter>
	</DialogContent>
</Dialog>
