<!--
  @component
  Run now (agent-run-modes-ux §1, RUN-4, RUN-9): starts one explicit turn, with an optional note. While a turn is open
  the run is Busy: the original turn is kept and nothing queues. When the agent can't start (paused, not configured,
  not signed in…) the reason is shown and Run stays off. A refusal from the office keeps the note and says why.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { textareaClass, helpClass, mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/office/api';
	import { membersApi } from '../../api';
	import { ageText } from '../../present';
	import type { MemberProfile, Turn } from '../../types';

	let {
		open = $bindable(false),
		organizationId,
		profile,
		onstarted,
		onrefused,
		onCloseAutoFocus
	}: {
		open: boolean;
		organizationId: string;
		profile: MemberProfile;
		onstarted: (turn: Turn) => void;
		onCloseAutoFocus?: (event: Event) => void;
		/** The office refused (busy, can't start): the profile reloads to show why. */
		onrefused?: () => void;
	} = $props();

	let note = $state('');
	let busy = $state(false);
	let refusal = $state<{ code: string; message: string } | undefined>();
	const ids = $props.id();

	/** Known before trying: an open turn (Busy) or a reason it can't start. */
	const blocked = $derived(
		profile.openTurn
			? `${profile.name} is already running a turn (started ${ageText(profile.openTurn.startedAt)}). It takes new input when that turn ends; nothing is queued.`
			: profile.cannotStart
	);

	function changeOpen(next: boolean) {
		if (!next && busy) open = true;
		else if (!next) refusal = undefined;
	}

	async function run(event: SubmitEvent) {
		event.preventDefault();
		if (busy || blocked) return;
		busy = true;
		refusal = undefined;
		try {
			const turn = await membersApi.runNow(organizationId, profile.id, note.trim());
			note = '';
			open = false;
			refusal = undefined;
			onstarted(turn);
		} catch (e) {
			const failure = toAppError(e);
			refusal = { code: failure.code ?? '', message: failure.message };
			if (failure.code === 'busy' || failure.code === 'cannot_start') onrefused?.();
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={changeOpen}>
	<DialogContent {onCloseAutoFocus} class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
		<form
			onsubmit={run}
			class="grid gap-4"
			aria-describedby={blocked ? `${ids}-blocked` : undefined}
		>
			<DialogHeader>
				<DialogTitle text="Run {profile.name} now" />
				<DialogDescription>
					Starts one turn with {profile.name}'s current setup. It works on what it has been given,
					and on your note if you write one.
				</DialogDescription>
			</DialogHeader>
			<div>
				<Label for="{ids}-note">What should {profile.name} do?</Label>
				<textarea
					id="{ids}-note"
					bind:value={note}
					rows="3"
					maxlength="2000"
					class="mt-2 {textareaClass}"
					aria-describedby="{ids}-help"></textarea>
				<p id="{ids}-help" class={helpClass}>Optional.</p>
			</div>
			{#if blocked}
				<div
					id="{ids}-blocked"
					class="rounded-md bg-white/5 p-3 text-sm/6 text-gray-300 ring-1 ring-white/10 ring-inset"
				>
					<p class="font-semibold text-white">{profile.openTurn ? 'Busy' : "Can't start"}</p>
					<p class="mt-1 [overflow-wrap:anywhere]">{blocked}</p>
					{#if !profile.openTurn && profile.status === 'active'}
						<a
							href={resolve('/settings/agents')}
							class="mt-2 inline-flex rounded-sm font-semibold text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
							>Open Agent setup</a
						>
					{/if}
				</div>
			{/if}
			{#if refusal && !blocked}
				<p class="text-sm/6 text-red-300 [overflow-wrap:anywhere]" role="alert">
					{refusal.code === 'busy'
						? 'Busy: '
						: refusal.code === 'cannot_start'
							? "Can't start: "
							: "Couldn't start the run: "}{refusal.message}
				</p>
			{/if}
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={busy}
					onclick={() => {
						open = false;
						changeOpen(false);
					}}>Cancel</Button
				>
				<Button
					type="submit"
					class={mobileButton}
					aria-busy={busy}
					disabled={!!blocked}
					aria-describedby={blocked ? `${ids}-blocked` : undefined}
					>{busy ? 'Starting…' : 'Run now'}</Button
				>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
