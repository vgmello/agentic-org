<!--
  @component
  The sign-in actions for one person on their profile (offices-and-teams-ux §7.4): Invite to sign in (no account yet),
  or New sign-in link and Remove access… (has an account). The owner's access can't be removed. The dialogs live in
  SignInAccessDialogs; this component decides what the profile does when an action ends: the profile reloads, then
  focus returns to the button that opened the action, or to the button that replaced it (Invite to sign in after
  Remove access), else the section heading. Never the body (app-shell-ux §6a).
  Usage: <SignInAccessActions {organizationId} organizationName="Lumen Labs" person={{ id, name, owner, canSignIn }}
    onchanged={reload} />
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { mobileButton } from '$lib/components/classes';
	import { focusIsFree } from '$lib/components/focus';
	import { Button } from '$lib/components/ui/button';
	import { toAppError } from '$lib/office/api';
	import SignInAccessDialogs, {
		NOT_IN_ORGANIZATION,
		type SignInAction,
		type SignInOutcome,
		type SignInPerson
	} from './SignInAccessDialogs.svelte';

	let {
		organizationId,
		organizationName,
		person,
		onchanged
	}: {
		organizationId: string;
		organizationName: string;
		person: SignInPerson;
		/** After a confirmed change the caller reloads; focus moves once that has finished. */
		onchanged: () => Promise<void>;
	} = $props();

	let host = $state<ReturnType<typeof SignInAccessDialogs>>();
	let issuing = $state<'invite' | 'reset' | undefined>();
	let refusal = $state<string | undefined>();
	let region: HTMLElement | null = null;
	/** Where the actions are, so focus can find the button that replaced the one that opened an action. */
	const holdRegion = (element: HTMLElement) => {
		region = element;
		return () => {
			region = null;
		};
	};

	async function act(kind: SignInAction, event: MouseEvent) {
		if (issuing) return;
		refusal = undefined;
		if (kind !== 'remove') issuing = kind;
		try {
			await host?.start(kind, person, event.currentTarget as HTMLElement);
		} catch (e) {
			const failure = toAppError(e);
			refusal = failure.code === 'not_found' ? NOT_IN_ORGANIZATION : failure.message;
			if (failure.code === 'not_found') await onchanged();
		} finally {
			issuing = undefined;
		}
	}

	async function settled(_memberId: string, outcome: SignInOutcome, from: HTMLElement | null) {
		if (outcome === 'gone') refusal = NOT_IN_ORGANIZATION;
		if (outcome !== 'cancelled') await onchanged();
		await tick();
		if (!focusIsFree(from)) return;
		if (from?.isConnected) return from.focus();
		const target =
			region?.querySelector<HTMLElement>('button') ??
			region?.closest('section')?.querySelector<HTMLElement>('h3');
		if (target?.matches('h3')) target.tabIndex = -1;
		target?.focus();
	}
</script>

<div {@attach holdRegion} class="flex flex-col gap-y-2">
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
				<p class="text-sm/6 text-fg-muted [overflow-wrap:anywhere]">
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
		<p class="text-xs/5 text-fg-muted">Inviting again cancels the previous link.</p>
	{/if}
	{#if refusal}
		<p class="text-sm/6 text-danger-text [overflow-wrap:anywhere]" role="alert">{refusal}</p>
	{/if}
</div>

<SignInAccessDialogs bind:this={host} {organizationId} {organizationName} onsettled={settled} />
