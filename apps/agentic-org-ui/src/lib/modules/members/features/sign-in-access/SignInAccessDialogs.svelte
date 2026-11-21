<!--
  @component
  The dialogs behind a person's sign-in actions (offices-and-teams-ux §7.4, SIGN-25, SIGN-36, SIGN-37): the one-time
  link, shown once, and the Remove access confirmation that names its effect. This is the one place that calls the
  members API's sign-in actions (inviteToSignIn, newSignInLink, removeAccess): the member profile and Settings › Members
  and access both use it.

  The caller mounts it once and starts an action with `start(kind, person, opener)`, so it can stay mounted while a list
  reloads. Every started action reports how it ended through `onsettled`, once, after its dialog has closed and its
  focus trap has let go: the caller then decides where focus goes.
  Usage: <SignInAccessDialogs bind:this={host} {organizationId} organizationName="Lumen Labs" onsettled={settled} />
-->
<script lang="ts" module>
	import type { OneTimeLinkValue } from './OneTimeLink.svelte';

	/** The person as the sign-in actions need them. */
	export interface SignInPerson {
		id: string;
		name: string;
		owner: boolean;
		canSignIn: boolean;
		invitePending?: { expiresAt: string };
	}

	export type SignInAction = 'invite' | 'reset' | 'remove';
	/**
	 * `linked`: a one-time link was shown and closed. `removed`: access was removed. `cancelled`: nothing changed.
	 * `gone`: the person is no longer in the organization, so nothing changed here.
	 */
	export type SignInOutcome = 'linked' | 'removed' | 'cancelled' | 'gone';

	/** Said when an action names a person who has left the organization since the page loaded. */
	export const NOT_IN_ORGANIZATION = "That person isn't in this organization any more.";
</script>

<script lang="ts">
	import { toast } from 'svelte-sonner';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { toAppError } from '$lib/modules/offices/office-api';
	import { membersApi } from '../../api';
	import OneTimeLink from './OneTimeLink.svelte';

	let {
		organizationId,
		organizationName,
		onsettled
	}: {
		organizationId: string;
		organizationName: string;
		/** Called once per started action, after its dialog has closed. */
		onsettled: (memberId: string, outcome: SignInOutcome, from: HTMLElement | null) => void;
	} = $props();

	// What the dialogs show. Kept after the outcome so a closing dialog never loses its names.
	let shown = $state.raw<SignInPerson | undefined>();
	let from: HTMLElement | null = null;
	let active = false;
	/** How the open action ended; reported once its dialog has closed. */
	let outcome: SignInOutcome | undefined;
	let link = $state.raw<OneTimeLinkValue | undefined>();
	let linkOpen = $state(false);
	let removeOpen = $state(false);

	const who = $derived(shown ? shown.name.split(/\s+/)[0] || shown.name : '');

	// Once the host is gone (the person navigated away while a dialog was closing), nothing is reported: the caller is
	// gone too, and moving focus or reloading then would act on the next page.
	let disposed = false;
	$effect(() => () => {
		disposed = true;
	});

	/**
	 * Starts an action on `person`. A link resolves once it is issued and shown; a removal resolves once it is asked
	 * for. A refusal rejects, nothing is shown, and `onsettled` is not called: the caller shows the refusal.
	 */
	export async function start(
		kind: SignInAction,
		person: SignInPerson,
		opener: HTMLElement | null
	): Promise<void> {
		shown = person;
		from = opener;
		outcome = undefined;
		active = true;
		if (kind === 'remove') {
			removeOpen = true;
			return;
		}
		try {
			const made =
				kind === 'invite'
					? await membersApi.inviteToSignIn(organizationId, person.id)
					: await membersApi.newSignInLink(organizationId, person.id);
			link = { ...made, kind };
			linkOpen = true;
		} catch (e) {
			active = false;
			throw e;
		}
	}

	// The one-time link is shown only once: Done, Escape or the close button end it.
	function finishLink() {
		if (!linkOpen) return;
		outcome = 'linked';
		linkOpen = false;
	}

	async function confirmRemove() {
		const person = shown;
		if (!person) return;
		try {
			await membersApi.removeAccess(organizationId, person.id);
		} catch (e) {
			if (toAppError(e).code === 'not_found') {
				outcome = 'gone';
				removeOpen = false;
				return;
			}
			// Any other refusal stays inside the confirmation, which stays open.
			throw e;
		}
		outcome = 'removed';
		toast.success(
			`Removed ${person.name}'s access. ${person.name.split(/\s+/)[0] || person.name} is signed out everywhere.`
		);
	}

	/**
	 * A dialog is closing. Focus belongs to the caller, so the dialog doesn't restore it; the link goes with the dialog.
	 * The dialog's content still holds focus while it animates out, so the caller hears how the action ended once that
	 * content is gone (at most a second later) and focus is free to move; never after the host has been torn down.
	 */
	function closed(event: Event) {
		event.preventDefault();
		link = undefined;
		const person = shown;
		if (!active || !person) return;
		active = false;
		const opener = from;
		const ended = outcome ?? 'cancelled';
		from = null;
		const holder = document.activeElement;
		const until = performance.now() + 1000;
		const settle = () => {
			if (disposed) return;
			if (holder?.isConnected && document.activeElement === holder && performance.now() < until)
				requestAnimationFrame(settle);
			else onsettled(person.id, ended, opener);
		};
		requestAnimationFrame(settle);
	}
</script>

<Dialog
	open={linkOpen}
	onOpenChange={(next) => {
		if (!next) finishLink();
	}}
>
	<DialogContent
		onCloseAutoFocus={closed}
		class="max-h-[calc(100dvh-2rem)] grid-cols-[minmax(0,1fr)] overflow-y-auto max-[639px]:max-w-[calc(100%-1rem)] max-[639px]:p-4"
	>
		<DialogHeader>
			<DialogTitle
				text={link?.kind === 'reset'
					? `New sign-in link for ${shown?.name ?? ''}`
					: `Invite ${shown?.name ?? ''} to sign in`}
			/>
		</DialogHeader>
		{#if link}<OneTimeLink {link} {who} />{/if}
		<DialogFooter>
			<Button type="button" class={mobileButton} onclick={finishLink}>Done</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>

<ConfirmDialog
	bind:open={removeOpen}
	title="Remove {who}'s access to {organizationName}?"
	description="{who} is signed out everywhere and can't open {organizationName}. {who} stays a person here."
	confirmLabel="Remove access"
	busyLabel="Removing access…"
	destructive
	onconfirm={confirmRemove}
	onCloseAutoFocus={closed}
/>
