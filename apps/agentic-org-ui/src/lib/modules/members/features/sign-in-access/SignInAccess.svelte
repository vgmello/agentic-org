<!--
  @component
  A person's sign-in access on their profile (offices-and-teams-ux §7.4, SIGN-25, SIGN-36, SIGN-37): whether they can
  sign in, and for the owner or the administrator Invite to sign in (no account yet), or New sign-in link and Remove
  access… (has an account). A link is shown once, in a dialog. The owner's access can't be removed. Agents have none
  of this: an account is never tied to an agent, so the caller renders it only for people.
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import DescriptionList from '$lib/components/DescriptionList.svelte';
	import Section from '$lib/components/Section.svelte';
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
	import { membersApi } from '../../api';
	import type { MemberProfile } from '../../types';
	import { expiryText } from './expiry';
	import OneTimeLink, { type OneTimeLinkValue } from './OneTimeLink.svelte';

	let {
		organizationId,
		organizationName,
		profile,
		onchanged
	}: {
		organizationId: string;
		organizationName: string;
		profile: MemberProfile;
		/** After a confirmed change: the profile reloads. */
		onchanged: () => Promise<void>;
	} = $props();

	/** "Sam" for "Send this link to Sam" (the spec's wording uses the first name). */
	const who = $derived(profile.name.split(/\s+/)[0] || profile.name);
	const canManage = $derived(profile.viewer.isOwnerOrAdmin && profile.status !== 'inactive');

	let issuing = $state<'invite' | 'reset' | undefined>();
	let refusal = $state<string | undefined>();
	let link = $state.raw<OneTimeLinkValue | undefined>();
	let linkOpen = $state(false);
	let removeOpen = $state(false);
	let opener: HTMLElement | null = null;

	async function issue(kind: 'invite' | 'reset', event: MouseEvent) {
		if (issuing) return;
		opener = event.currentTarget as HTMLElement;
		issuing = kind;
		refusal = undefined;
		try {
			const made =
				kind === 'invite'
					? await membersApi.inviteToSignIn(organizationId, profile.id)
					: await membersApi.newSignInLink(organizationId, profile.id);
			link = { ...made, kind };
			// The profile reloads only after the link is closed: a failed reload must never take the one-time link away.
			linkOpen = true;
		} catch (e) {
			refusal = toAppError(e).message;
		} finally {
			issuing = undefined;
		}
	}

	function closeLink() {
		linkOpen = false;
		void onchanged();
	}

	async function removeAccess() {
		await membersApi.removeAccess(organizationId, profile.id);
		toast.success(`Removed ${profile.name}'s access. ${who} is signed out everywhere.`);
		await onchanged();
	}

	let region = $state<HTMLElement | null>(null);

	function restore(event: Event) {
		event.preventDefault();
		// The one-time link is gone once its dialog has closed: it's shown only once.
		link = undefined;
		// The button that opened it may be gone (Remove access…, or Invite after an account exists): the section heading.
		if (opener?.isConnected) return opener.focus();
		const heading = region?.querySelector<HTMLElement>('h3');
		if (heading) {
			heading.tabIndex = -1;
			heading.focus();
		}
	}

	const status = $derived(
		profile.canSignIn
			? 'Can sign in'
			: profile.invitePending
				? `No account yet. Invited: the link works until ${expiryText(profile.invitePending.expiresAt).replace(/^at /, '')}.`
				: 'No account yet'
	);
</script>

<div bind:this={region}>
	<Section
		level={3}
		title="Sign-in access"
		description="An account lets a person sign in to {organizationName} in the browser. The product sends no email."
	>
		<DescriptionList items={[{ term: 'Access', value: status }]} />
		{#if canManage}
			<div class="flex flex-wrap gap-3 px-4 pt-4 min-[640px]:px-6 min-[1024px]:px-8">
				{#if !profile.canSignIn}
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
					{#if profile.owner}
						<p class="text-sm/6 text-gray-400">
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
			{#if profile.invitePending && !profile.canSignIn}
				<p class="mt-2 px-4 text-xs/5 text-gray-400 min-[640px]:px-6 min-[1024px]:px-8">
					Inviting again cancels the previous link.
				</p>
			{/if}
			{#if refusal}
				<p
					class="mt-3 px-4 text-sm/6 text-red-300 [overflow-wrap:anywhere] min-[640px]:px-6 min-[1024px]:px-8"
					role="alert"
				>
					{refusal}
				</p>
			{/if}
		{/if}
	</Section>
</div>

<Dialog bind:open={linkOpen} onOpenChange={(next) => !next && void onchanged()}>
	<DialogContent
		onCloseAutoFocus={restore}
		class="max-h-[calc(100dvh-2rem)] grid-cols-[minmax(0,1fr)] overflow-y-auto max-[639px]:max-w-[calc(100%-1rem)] max-[639px]:p-4"
	>
		<DialogHeader>
			<DialogTitle
				text={link?.kind === 'reset'
					? `New sign-in link for ${profile.name}`
					: `Invite ${profile.name} to sign in`}
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
	onconfirm={removeAccess}
	onCloseAutoFocus={restore}
/>
