<!--
  @component
  Run now (agent-run-modes-ux §1, RUN-4, RUN-9): starts one explicit turn, with an optional note. While a turn is open
  the run is Busy: the original turn is kept and nothing queues. When the agent can't start (paused, not configured,
  not signed in…) the reason is shown and Run stays off. A refusal from the office keeps the note and says why.
  Mount it in a {#key} that changes on each opening.
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
	import { busyText } from '../../present';
	import type { MemberProfile, RunNowResult } from '../../types';

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
		/** HQ recorded the request: the turn (accepted until the office confirms) and the profile after it. */
		onstarted: (result: RunNowResult) => void;
		onCloseAutoFocus?: (event: Event) => void;
		/** The office refused (busy, can't start): the profile reloads to show why. */
		onrefused?: () => void;
	} = $props();

	let note = $state('');
	let busy = $state(false);
	let refusal = $state<{ code: string; message: string } | undefined>();
	const ids = $props.id();

	/**
	 * This opening's own request went through: the dialog is closing, and the open turn the new profile shows is that
	 * request, not a reason to say Busy. The caller mounts a fresh dialog ({#key}) for each opening.
	 */
	let requested = $state(false);

	/** Known before trying: an open turn (Busy) or a reason it can't start. */
	const blocked = $derived(
		requested
			? undefined
			: profile.openTurn
				? busyText(profile.name, profile.openTurn)
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
			const result = await membersApi.runNow(organizationId, profile.id, note.trim());
			note = '';
			requested = true;
			open = false;
			refusal = undefined;
			onstarted(result);
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
	<DialogContent
		{onCloseAutoFocus}
		class="max-h-[calc(100dvh-2rem)] grid-cols-[minmax(0,1fr)] overflow-y-auto max-[639px]:max-w-[calc(100%-1rem)] max-[639px]:p-4"
	>
		<form
			onsubmit={run}
			class="grid gap-4"
			aria-describedby={blocked ? `${ids}-blocked` : undefined}
		>
			<DialogHeader class="pr-8">
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
					disabled={busy}
					class="mt-2 {textareaClass}"
					aria-describedby="{ids}-help"></textarea>
				<p id="{ids}-help" class={helpClass}>Optional.</p>
			</div>
			{#if blocked}
				<div
					id="{ids}-blocked"
					class="rounded-md bg-surface-raised p-3 text-sm/6 text-fg-secondary ring-1 ring-line ring-inset"
				>
					<p class="font-semibold text-fg">{profile.openTurn ? 'Busy' : "Can't start"}</p>
					<p class="mt-1 [overflow-wrap:anywhere]">{blocked}</p>
					{#if !profile.openTurn && profile.status === 'active'}
						<a
							href={resolve('/settings/agents')}
							class="mt-2 inline-flex rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
							>Open Agent setup</a
						>
					{/if}
				</div>
			{/if}
			{#if refusal && !blocked}
				<p class="text-sm/6 text-danger-text [overflow-wrap:anywhere]" role="alert">
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
					>{busy ? 'Requesting…' : 'Run now'}</Button
				>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
