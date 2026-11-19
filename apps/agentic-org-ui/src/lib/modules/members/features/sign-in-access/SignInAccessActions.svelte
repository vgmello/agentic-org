<!--
  @component
  The sign-in access actions for one person (offices-and-teams-ux §7.4, SIGN-25, SIGN-36, SIGN-37): Invite to sign in
  (no account yet), or New sign-in link and Remove access… (has an account). The one-time link is shown once, in a
  dialog; Remove access asks first and names its effect. The owner's access can't be removed.
  The caller decides who may see these actions and passes the calls, so the members profile and Settings › Members and
  access share this component with their own data: `issueLink` and `removeAccess` answer or refuse, and nothing here
  keeps a local copy of the result.
  Usage:
    <SignInAccessActions
      organizationName="Lumen Labs"
      person={{ id, name, owner, canSignIn, invitePending }}
      issueLink={(memberId, kind) => api.issueLink(orgId, memberId, kind)}
      removeAccess={(memberId) => api.removeAccess(orgId, memberId)}
      onchanged={reload}
    />
-->
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
	import OneTimeLink, { type OneTimeLinkValue } from './OneTimeLink.svelte';

	/** The person as the sign-in actions need them. */
	export interface SignInPerson {
		id: string;
		name: string;
		owner: boolean;
		canSignIn: boolean;
		invitePending?: { expiresAt: string };
	}

	let {
		organizationName,
		person,
		issueLink,
		removeAccess,
		onchanged
	}: {
		organizationName: string;
		person: SignInPerson;
		/** A one-time link for this person. Refusals are thrown and shown inline. */
		issueLink: (
			memberId: string,
			kind: OneTimeLinkValue['kind']
		) => Promise<Omit<OneTimeLinkValue, 'kind'>>;
		removeAccess: (memberId: string) => Promise<void>;
		/** After a confirmed change, the caller reloads. Runs after the link dialog closes, never before. */
		onchanged: () => Promise<void>;
	} = $props();

	/** "Sam" for "Send this link to Sam" (the spec's wording uses the first name). */
	const who = $derived(person.name.split(/\s+/)[0] || person.name);

	let issuing = $state<'invite' | 'reset' | undefined>();
	let refusal = $state<string | undefined>();
	let link = $state.raw<OneTimeLinkValue | undefined>();
	let linkOpen = $state(false);
	let removeOpen = $state(false);
	let opener: HTMLElement | null = null;
	let region = $state<HTMLElement | null>(null);

	async function issue(kind: 'invite' | 'reset', event: MouseEvent) {
		if (issuing) return;
		opener = event.currentTarget as HTMLElement;
		issuing = kind;
		refusal = undefined;
		try {
			const made = await issueLink(person.id, kind);
			link = { ...made, kind };
			// The caller reloads only after the link is closed: a failed reload must never take the one-time link away.
			linkOpen = true;
		} catch (e) {
			const failure = toAppError(e);
			refusal = failure.message;
			// The person has left the organization since the page loaded: the caller's list is out of date.
			if (failure.code === 'not_found') void onchanged();
		} finally {
			issuing = undefined;
		}
	}

	function closeLink() {
		linkOpen = false;
		void onchanged();
	}

	async function confirmRemove() {
		try {
			await removeAccess(person.id);
		} catch (e) {
			// A refusal stays in the confirmation (it stays open); a person who has gone reloads the list.
			if (toAppError(e).code === 'not_found') await onchanged();
			throw e;
		}
		toast.success(`Removed ${person.name}'s access. ${who} is signed out everywhere.`);
		await onchanged();
	}

	function restore(event: Event) {
		event.preventDefault();
		// The one-time link is gone once its dialog has closed: it's shown only once.
		link = undefined;
		// The button that opened it may be gone (Remove access…, or Invite after an account exists): the section heading.
		if (opener?.isConnected) return opener.focus();
		const heading = region?.closest('section')?.querySelector<HTMLElement>('h3');
		const row = region?.closest<HTMLElement>('[data-access-row]');
		const target = heading ?? row;
		if (target) {
			target.tabIndex = -1;
			target.focus();
		}
	}
</script>

<div bind:this={region} class="flex flex-col gap-y-2">
	<div class="flex flex-wrap gap-3">
		{#if !person.canSignIn}
			<Button
				variant="outline"
				class={mobileButton}
				aria-busy={issuing === 'invite'}
				onclick={(e) => issue('invite', e)}
				>{issuing === 'invite' ? 'Inviting…' : 'Invite to sign in'}</Button
			>
		{:else}
			<Button
				variant="outline"
				class={mobileButton}
				aria-busy={issuing === 'reset'}
				onclick={(e) => issue('reset', e)}
				>{issuing === 'reset' ? 'Making link…' : 'New sign-in link'}</Button
			>
			{#if person.owner}
				<p class="text-sm/6 text-gray-400 [overflow-wrap:anywhere]">
					Remove access isn't available for the owner. Hand over ownership first.
				</p>
			{:else}
				<Button
					variant="outline"
					class={mobileButton}
					onclick={(e) => {
						opener = e.currentTarget;
						removeOpen = true;
					}}>Remove access…</Button
				>
			{/if}
		{/if}
	</div>
	{#if person.invitePending && !person.canSignIn}
		<p class="text-xs/5 text-gray-400">Inviting again cancels the previous link.</p>
	{/if}
	{#if refusal}
		<p class="text-sm/6 text-red-300 [overflow-wrap:anywhere]" role="alert">{refusal}</p>
	{/if}
</div>

<Dialog bind:open={linkOpen} onOpenChange={(next) => !next && void onchanged()}>
	<DialogContent
		onCloseAutoFocus={restore}
		class="max-h-[calc(100dvh-2rem)] grid-cols-[minmax(0,1fr)] overflow-y-auto max-[639px]:max-w-[calc(100%-1rem)] max-[639px]:p-4"
	>
		<DialogHeader>
			<DialogTitle
				text={link?.kind === 'reset'
					? `New sign-in link for ${person.name}`
					: `Invite ${person.name} to sign in`}
			/>
		</DialogHeader>
		{#if link}<OneTimeLink {link} {who} />{/if}
		<DialogFooter>
			<Button type="button" class={mobileButton} onclick={closeLink}>Done</Button>
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
	onCloseAutoFocus={restore}
/>
