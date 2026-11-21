<!--
  @component
  Approve a join (offices-and-teams-ux §6.3, OFC-4..6): type the 8-character check code shown on the joining machine. Approve is
  enabled once the code has 8 characters; a wrong code says how many tries are left, and the third cancels the invitation. Decline
  asks once, in the same dialog.
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { toAppError } from '$lib/modules/offices/office-api';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { errorClass, helpClass, inputClass, mobileButton } from '$lib/components/classes';
	import { officesApi } from '../../api';
	import { CODE_HINT, groupCode, readCheckCode } from '../../check-code';
	import { agoText, madeAtText } from '../../time';
	import type { JoinRequest } from '../../types';

	let {
		open = $bindable(false),
		request,
		organizationId,
		organizationName,
		onchanged,
		onCloseAutoFocus
	}: {
		open: boolean;
		request: JoinRequest | undefined;
		organizationId: string;
		organizationName: string;
		/** The list changed (approved, declined, or the invitation was cancelled): read it again. */
		onchanged: () => void | Promise<void>;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	type Stage = 'approve' | 'decline' | 'ended';
	let stage = $state<Stage>('approve');
	let typed = $state('');
	let hint = $state(false);
	let error = $state<string | undefined>();
	let busy = $state<'approve' | 'decline' | undefined>();
	let input = $state<HTMLInputElement | null>(null);
	// The request is kept while the dialog closes, so its words don't vanish mid-animation.
	let shown = $state<JoinRequest | undefined>();
	$effect(() => {
		// Only opening resets the dialog: the list reloading behind it (a wrong code) must not clear its stage or what was typed.
		if (open)
			untrack(() => {
				if (!request) return;
				shown = request;
				stage = 'approve';
				typed = '';
				hint = false;
				error = undefined;
			});
	});
	const read = $derived(readCheckCode(typed));
	const name = $derived(shown?.officeName ?? 'this office');
	const created = $derived(shown ? madeAtText(shown.invitationCreatedAt, Date.now()) : '');

	function change(event: Event & { currentTarget: HTMLInputElement }) {
		const next = readCheckCode(event.currentTarget.value);
		hint = next.rejected;
		error = undefined;
		typed = groupCode(next.code);
		event.currentTarget.value = typed;
	}

	async function approve(event: SubmitEvent) {
		event.preventDefault();
		if (!shown || busy || !read.complete) return;
		busy = 'approve';
		error = undefined;
		try {
			await officesApi.approveJoin(organizationId, shown.id, read.code);
			open = false;
			toast.success(`${shown.officeName} joined ${organizationName}.`);
			await onchanged();
		} catch (e) {
			const failure = toAppError(e);
			error = failure.message;
			if (failure.code === 'join_cancelled') {
				stage = 'ended';
				await onchanged();
			} else {
				typed = '';
				input?.focus();
			}
		} finally {
			busy = undefined;
		}
	}

	async function decline() {
		if (!shown || busy) return;
		busy = 'decline';
		error = undefined;
		try {
			await officesApi.declineJoin(organizationId, shown.id);
			open = false;
			await onchanged();
		} catch (e) {
			error = toAppError(e).message;
		} finally {
			busy = undefined;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent
		{onCloseAutoFocus}
		class="max-h-[calc(100dvh-2rem)] min-w-0 overflow-y-auto sm:max-w-lg"
	>
		{#if stage === 'decline'}
			<DialogHeader>
				<DialogTitle text={`Decline ${name}?`} />
				<DialogDescription>Its invitation is cancelled.</DialogDescription>
			</DialogHeader>
			{#if error}<p class="text-sm/6 text-danger-text" role="alert">{error}</p>{/if}
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={!!busy}
					onclick={() => (stage = 'approve')}>Back</Button
				>
				<Button
					type="button"
					class={mobileButton}
					aria-busy={busy === 'decline'}
					aria-disabled={!!busy}
					onclick={decline}>{busy === 'decline' ? 'Declining…' : 'Decline'}</Button
				>
			</DialogFooter>
		{:else if stage === 'ended'}
			<DialogHeader>
				<DialogTitle text={`${name}'s invitation is cancelled`} />
			</DialogHeader>
			<p class="text-sm/6 text-danger-text [overflow-wrap:anywhere]" role="alert">{error}</p>
			<DialogFooter
				><Button type="button" class={mobileButton} onclick={() => (open = false)}>Close</Button
				></DialogFooter
			>
		{:else}
			<form class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-6" onsubmit={approve} novalidate>
				<DialogHeader>
					<DialogTitle text={`Approve ${name}?`} />
					<DialogDescription class="[overflow-wrap:anywhere]">
						A computer at {shown?.fromAddress} used the invitation for {name} (created {created})
						{shown ? agoText(shown.requestedAt, Date.now()) : ''}.
					</DialogDescription>
				</DialogHeader>
				<div>
					<Label for="check-code" class="block text-sm/6 font-medium text-fg"
						>Type the check code shown on {name}</Label
					>
					<div class="mt-2">
						<Input
							bind:ref={input}
							id="check-code"
							value={typed}
							oninput={change}
							autocomplete="off"
							autocapitalize="characters"
							spellcheck={false}
							aria-invalid={error || hint ? true : undefined}
							aria-describedby="check-code-help{hint ? ' check-code-hint' : ''}{error
								? ' check-code-error'
								: ''}"
							size={1}
							class="{inputClass} w-full min-w-0 font-mono tracking-widest"
						/>
					</div>
					{#if hint}<p id="check-code-hint" class={errorClass}>{CODE_HINT}</p>{/if}
					{#if error}<p id="check-code-error" class={errorClass} role="alert">{error}</p>{/if}
					<p id="check-code-help" class={helpClass}>
						Verify the code on the destination terminal or through your trusted authenticated
						session to that machine. Decline unsolicited or unverified relayed codes.
					</p>
				</div>
				<DialogFooter>
					<Button
						type="button"
						variant="outline"
						class={mobileButton}
						disabled={!!busy}
						onclick={() => (stage = 'decline')}>Decline</Button
					>
					<Button
						type="submit"
						class="{mobileButton} [overflow-wrap:anywhere]"
						disabled={!read.complete}
						aria-busy={busy === 'approve'}
						aria-disabled={!!busy}>{busy === 'approve' ? 'Approving…' : `Approve ${name}`}</Button
					>
				</DialogFooter>
			</form>
		{/if}
	</DialogContent>
</Dialog>
