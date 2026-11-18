<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { onMount, tick } from 'svelte';
	import { appSession } from './app-session.svelte';
	import {
		credentialCopy,
		describeCredentialFailure,
		type CredentialFailure
	} from './credential-copy';
	import { checkSignIn } from './credential-rules';
	import { drafts } from './draft-registry';
	import PasswordField from './PasswordField.svelte';
	import { signInHref } from './return-target';
	import { signOutFlow } from './sign-out-flow.svelte';

	/*
	 * The tab's session interruptions, over whatever page is open:
	 * - "Sign in again" (SIGN-10, SIGN-14): opens when HQ refuses a request because the session
	 *   ended. Signing in from any tab broadcasts, every tab re-reads the shared record, and the
	 *   dialog closes everywhere because the state is signed in again.
	 * - Not now (SIGN-15): closes the dialog and leaves a bar; the copy depends on whether the
	 *   page registered editable fields in the draft registry.
	 * - Sign out confirmation (SIGN-19) and its failure (SIGN-29).
	 */
	const current = $derived(appSession.state);
	const login = $derived(current.kind === 'signed-in' ? current.session.account.login : '');
	// The sign-in and registration pages are the alternative to the dialog, never under it.
	const elsewhere = $derived(page.url.pathname === '/sign-in' || page.url.pathname === '/register');
	const ended = $derived(
		!elsewhere && current.kind === 'signed-in' && current.reauth !== undefined
	);
	let dismissed = $state(false);
	let editable = $state(false);
	let password = $state('');
	let passwordError = $state<string>();
	let failure = $state<CredentialFailure>();
	let busy = $state(false);
	let discardAsk = $state(false);
	let passwordInput = $state<HTMLInputElement | null>(null);
	let dialog = $state<HTMLDialogElement | null>(null);
	let confirmDialog = $state<HTMLDialogElement | null>(null);
	let returnFocus: Element | null = null;

	onMount(() => signOutFlow.listen());

	$effect(() => {
		if (!ended && !elsewhere) {
			dismissed = false;
			discardAsk = false;
			password = '';
			failure = passwordError = undefined;
		}
	});
	$effect(() => {
		const open = ended && !dismissed;
		if (!dialog) return;
		if (open && !dialog.open) {
			editable = drafts.hasEditable();
			returnFocus = document.activeElement;
			dialog.showModal();
			void tick().then(() => passwordInput?.focus());
		} else if (!open && dialog.open) {
			dialog.close();
			if (!dismissed && returnFocus instanceof HTMLElement) returnFocus.focus();
		}
	});
	$effect(() => {
		if (!confirmDialog) return;
		if (signOutFlow.confirming && !confirmDialog.open) confirmDialog.showModal();
		else if (!signOutFlow.confirming && confirmDialog.open) confirmDialog.close();
	});

	function notNow() {
		dismissed = true;
		discardAsk = false;
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		failure = passwordError = undefined;
		const problems = checkSignIn(login, password).filter((p) => p.field === 'password');
		if (problems.length > 0) {
			passwordError = credentialCopy('password', problems[0].rule);
			passwordInput?.focus();
			return;
		}
		busy = true;
		try {
			await appSession.controller().signIn({ login, password });
			password = '';
		} catch (error) {
			failure = describeCredentialFailure(error, Date.now());
			if (failure.kind === 'mismatch') {
				password = '';
				await tick();
				passwordInput?.focus();
			}
		} finally {
			busy = false;
		}
	}

	async function someoneElse(discard = false) {
		if (!discard && drafts.hasUnsavedText()) {
			discardAsk = true;
			return;
		}
		dialog?.close();
		dismissed = true;
		await goto(resolve(signInHref(page.url.pathname + page.url.search, location.origin, 'ended')));
	}
</script>

{#if ended && dismissed}
	<div
		role="status"
		class="fixed inset-x-0 top-0 z-40 flex items-center justify-center gap-x-4 bg-amber-500/15 px-4 py-2 text-sm/6 text-amber-100 backdrop-blur"
	>
		<p>
			{editable
				? "You're signed out. Text you haven't saved is still here: copy it, or sign in to save it."
				: "You're signed out."}
		</p>
		<button type="button" class="font-semibold underline" onclick={() => (dismissed = false)}
			>Sign in</button
		>
	</div>
{/if}

<dialog
	bind:this={dialog}
	aria-labelledby="sign-in-again-title"
	aria-describedby="sign-in-again-reason"
	oncancel={(event) => {
		event.preventDefault();
		notNow();
	}}
	class="m-auto w-full max-w-md rounded-lg bg-gray-900 p-6 text-white outline-1 outline-white/10 backdrop:bg-gray-950/80"
>
	{#if ended}
		<h2 id="sign-in-again-title" class="text-lg/7 font-semibold">Sign in again</h2>
		<div id="sign-in-again-reason" class="mt-2 text-sm/6 text-gray-300">
			<p>Your session ended. Sign in again to carry on.</p>
			{#if editable}<p>Text you haven't saved stays on this page.</p>{/if}
		</div>
		{#if discardAsk}
			<div
				role="alertdialog"
				aria-labelledby="discard-title"
				class="mt-4 rounded-md bg-white/5 p-3"
			>
				<p id="discard-title" class="text-sm/6">Discard text you haven't saved on this page?</p>
				<div class="mt-3 flex justify-end gap-x-3">
					<button
						type="button"
						class="rounded-md bg-white/10 px-3 py-1.5 text-sm font-semibold"
						onclick={() => (discardAsk = false)}>Keep editing</button
					>
					<button
						type="button"
						class="rounded-md bg-red-500 px-3 py-1.5 text-sm font-semibold"
						onclick={() => someoneElse(true)}>Discard and sign in</button
					>
				</div>
			</div>
		{/if}
		<div aria-live="assertive">
			{#if failure}
				<div
					id="sign-in-again-error"
					role="alert"
					class="mt-4 border-l-4 border-red-400 bg-red-500/10 p-3 text-sm/6 text-red-200"
				>
					<p>{failure.message}</p>
					{#if failure.kind === 'out_of_date'}<button
							type="button"
							class="mt-2 font-semibold underline"
							onclick={() => location.reload()}>Reload</button
						>{/if}
				</div>
			{/if}
		</div>
		<form
			class="mt-4 space-y-4"
			novalidate
			onsubmit={submit}
			aria-describedby={failure ? 'sign-in-again-error' : undefined}
		>
			<p class="text-sm/6">Account: <span class="font-semibold">{login}</span></p>
			<input type="text" name="username" autocomplete="username" value={login} readonly hidden />
			<PasswordField
				id="sign-in-again-password"
				label="Password"
				autocomplete="current-password"
				bind:value={password}
				bind:input={passwordInput}
				error={passwordError}
				readonly={busy}
			/>
			<div class="flex items-center gap-x-3">
				<button
					type="button"
					class="mr-auto text-sm/6 font-semibold text-indigo-300 underline"
					onclick={() => someoneElse()}>Sign in as someone else</button
				>
				<button
					type="button"
					class="rounded-md bg-white/10 px-3 py-1.5 text-sm font-semibold"
					onclick={notNow}>Not now</button
				>
				<button
					type="submit"
					disabled={busy}
					class="rounded-md bg-indigo-500 px-3 py-1.5 text-sm font-semibold hover:bg-indigo-600 disabled:opacity-60"
					>{busy ? 'Signing in…' : 'Sign in'}</button
				>
			</div>
		</form>
	{/if}
</dialog>

<dialog
	bind:this={confirmDialog}
	aria-labelledby="sign-out-confirm-title"
	oncancel={(event) => {
		event.preventDefault();
		signOutFlow.stay();
	}}
	class="m-auto w-full max-w-md rounded-lg bg-gray-900 p-6 text-white outline-1 outline-white/10 backdrop:bg-gray-950/80"
>
	<h2 id="sign-out-confirm-title" class="text-sm/6">
		Sign out? Text you haven't saved in any open tab of this HQ will be lost.
	</h2>
	<div class="mt-4 flex justify-end gap-x-3">
		<button
			type="button"
			class="rounded-md bg-white/10 px-3 py-1.5 text-sm font-semibold"
			onclick={() => signOutFlow.stay()}>Stay</button
		>
		<button
			type="button"
			class="rounded-md bg-red-500 px-3 py-1.5 text-sm font-semibold"
			onclick={() => signOutFlow.confirm()}>Sign out</button
		>
	</div>
</dialog>

{#if signOutFlow.failed && !elsewhere}
	<div
		role="alert"
		class="fixed inset-x-0 top-0 z-40 flex items-center justify-center gap-x-4 bg-red-500/15 px-4 py-2 text-sm/6 text-red-200"
	>
		<p>Couldn't sign out: HQ can't be reached. Try again when it's back.</p>
		<button type="button" class="font-semibold underline" onclick={() => signOutFlow.request()}
			>Try again</button
		>
	</div>
{/if}
