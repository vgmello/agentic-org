<!--
  @component
  The dialogs behind a person's sign-in actions (offices-and-teams-ux §7.4, SIGN-25, SIGN-36, SIGN-37): the one-time
  link, shown once, and the Remove access confirmation that names its effect. The host is mounted once by its caller and
  started with `start(kind, person, opener)`, so the caller can keep it mounted while a list reloads: a person who moves
  from one list to another keeps the dialog (and its focus) alive. Every started action reports how it ended through
  `onsettled`, once. The caller decides where focus goes.
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
	import { toAppError } from '$lib/office/api';
	import OneTimeLink from './OneTimeLink.svelte';

	let {
		organizationName,
		issueLink,
		removeAccess,
		onsettled
	}: {
		organizationName: string;
		/** A one-time link for this person. A refusal rejects and is shown by the caller. */
		issueLink: (
			memberId: string,
			kind: OneTimeLinkValue['kind']
		) => Promise<Omit<OneTimeLinkValue, 'kind'>>;
		removeAccess: (memberId: string) => Promise<void>;
		onsettled: (memberId: string, outcome: SignInOutcome, from: HTMLElement | null) => void;
	} = $props();

	// What the dialogs show. Kept after the outcome so a closing dialog never loses its names.
	let shown = $state.raw<SignInPerson | undefined>();
	let from: HTMLElement | null = null;
	let active = false;
	let link = $state.raw<OneTimeLinkValue | undefined>();
	let linkOpen = $state(false);
	let removeOpen = $state(false);

	const who = $derived(shown ? shown.name.split(/\s+/)[0] || shown.name : '');

	/**
	 * Starts an action on `person`. A link resolves once it is issued and shown; a removal resolves once it is asked
	 * for. A refusal rejects, and nothing is shown.
	 */
	export async function start(
		kind: SignInAction,
		person: SignInPerson,
		opener: HTMLElement | null
	): Promise<void> {
		shown = person;
		from = opener;
		active = true;
		if (kind === 'remove') {
			removeOpen = true;
			return;
		}
		try {
			const made = await issueLink(person.id, kind);
			link = { ...made, kind };
			linkOpen = true;
		} catch (e) {
			active = false;
			throw e;
		}
	}

	function settle(outcome: SignInOutcome) {
		if (!active || !shown) return;
		active = false;
		const opener = from;
		from = null;
		onsettled(shown.id, outcome, opener);
	}

	// The one-time link is shown only once: it goes when its dialog closes, and the outcome follows.
	function finishLink() {
		if (!linkOpen) return;
		linkOpen = false;
		link = undefined;
		settle('linked');
	}

	async function confirmRemove() {
		const person = shown;
		if (!person) return;
		try {
			await removeAccess(person.id);
		} catch (e) {
			if (toAppError(e).code === 'not_found') {
				removeOpen = false;
				settle('gone');
				return;
			}
			// Any other refusal stays inside the confirmation, which stays open.
			throw e;
		}
		toast.success(
			`Removed ${person.name}'s access. ${person.name.split(/\s+/)[0] || person.name} is signed out everywhere.`
		);
		settle('removed');
	}

	// Focus belongs to the caller: the dialogs never restore it themselves.
	const keepFocusWithCaller = (event: Event) => event.preventDefault();
</script>

<Dialog
	open={linkOpen}
	onOpenChange={(next) => {
		if (!next) finishLink();
	}}
>
	<DialogContent
		onCloseAutoFocus={keepFocusWithCaller}
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
	onCloseAutoFocus={(event) => {
		keepFocusWithCaller(event);
		settle('cancelled');
	}}
/>
