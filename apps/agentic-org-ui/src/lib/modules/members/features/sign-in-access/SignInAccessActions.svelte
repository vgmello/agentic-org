<!--
  @component
  The sign-in actions for one person on their profile (offices-and-teams-ux §7.4): Invite to sign in (no account yet),
  or New sign-in link and Remove access… (has an account). The owner's access can't be removed. The dialogs live in
  SignInAccessDialogs; this component decides what the profile does when an action ends. The caller passes the calls.
  Usage:
    <SignInAccessActions organizationName="Lumen Labs" person={{ id, name, owner, canSignIn }}
      issueLink={(id, kind) => api.issueLink(orgId, id, kind)} removeAccess={(id) => api.removeAccess(orgId, id)}
      onchanged={reload} />
-->
<script lang="ts">
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { toAppError } from '$lib/office/api';
	import SignInAccessDialogs, {
		type SignInAction,
		type SignInOutcome,
		type SignInPerson
	} from './SignInAccessDialogs.svelte';
	import type { OneTimeLinkValue } from './OneTimeLink.svelte';

	let {
		organizationName,
		person,
		issueLink,
		removeAccess,
		onchanged
	}: {
		organizationName: string;
		person: SignInPerson;
		issueLink: (
			memberId: string,
			kind: OneTimeLinkValue['kind']
		) => Promise<Omit<OneTimeLinkValue, 'kind'>>;
		removeAccess: (memberId: string) => Promise<void>;
		/** After a confirmed change the caller reloads. Runs once the focus has moved, never before. */
		onchanged: () => Promise<void>;
	} = $props();

	let host = $state<ReturnType<typeof SignInAccessDialogs>>();
	let issuing = $state<'invite' | 'reset' | undefined>();
	let refusal = $state<string | undefined>();
	let region = $state<HTMLElement | null>(null);

	async function act(kind: SignInAction, event: MouseEvent) {
		if (issuing) return;
		refusal = undefined;
		if (kind !== 'remove') issuing = kind;
		try {
			await host?.start(kind, person, event.currentTarget as HTMLElement);
		} catch (e) {
			const failure = toAppError(e);
			refusal = failure.message;
			if (failure.code === 'not_found') await onchanged();
		} finally {
			issuing = undefined;
		}
	}

	async function settled(_memberId: string, outcome: SignInOutcome, from: HTMLElement | null) {
		// The control that opened the action may be gone after the reload (Remove access… once the account is removed):
		// then the section heading takes focus.
		if (from?.isConnected) from.focus();
		else {
			const heading = region?.closest('section')?.querySelector<HTMLElement>('h3');
			if (heading) {
				heading.tabIndex = -1;
				heading.focus();
			}
		}
		if (outcome === 'linked' || outcome === 'removed') await onchanged();
		if (outcome === 'gone') {
			refusal = "That person isn't in this organization any more.";
			await onchanged();
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
				onclick={(e) => act('invite', e)}
				>{issuing === 'invite' ? 'Inviting…' : 'Invite to sign in'}</Button
			>
		{:else}
			<Button
				variant="outline"
				class={mobileButton}
				aria-busy={issuing === 'reset'}
				onclick={(e) => act('reset', e)}
				>{issuing === 'reset' ? 'Making link…' : 'New sign-in link'}</Button
			>
			{#if person.owner}
				<p class="text-sm/6 text-gray-400 [overflow-wrap:anywhere]">
					Remove access isn't available for the owner. Hand over ownership first.
				</p>
			{:else}
				<Button variant="outline" class={mobileButton} onclick={(e) => act('remove', e)}
					>Remove access…</Button
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

<SignInAccessDialogs
	bind:this={host}
	{organizationName}
	{issueLink}
	{removeAccess}
	onsettled={settled}
/>
