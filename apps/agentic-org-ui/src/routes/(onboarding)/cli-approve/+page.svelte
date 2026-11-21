<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { onDestroy, onMount, tick } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import { describeCliApprovalFailure } from '$lib/platform/auth/cli-approve';
	import {
		credentialCopy,
		describeCredentialFailure,
		LINK_USED_COPY,
		type CredentialFailure
	} from '$lib/platform/auth/credential-copy';
	import { checkSignIn } from '$lib/platform/auth/credential-rules';
	import { takeLinkSecret } from '$lib/platform/auth/link-fragment';
	import PasswordField from '$lib/platform/auth/PasswordField.svelte';

	// Kept strictly in this non-reactive closure after mount: never rendered, logged, stored, or
	// placed in URL query params. `takeLinkSecret` clears `location.hash` via `history.replaceState`.
	let secret: string | undefined;
	let linkPresent = $state(true);
	let phase = $state<'loading' | 'ready' | 'approved' | 'declined' | 'expired' | 'unknown'>(
		'loading'
	);
	let errorMessage = $state<string>();
	let deciding = $state<'approve' | 'decline'>();

	// Inline sign-in state used only when auth_enabled === true and browser is signed out.
	let login = $state('');
	let password = $state('');
	let loginError = $state<string>();
	let passwordError = $state<string>();
	let signInFailure = $state<CredentialFailure>();
	let signingIn = $state(false);
	let loginInput = $state<HTMLInputElement | null>(null);
	let passwordInput = $state<HTMLInputElement | null>(null);

	const address = $derived(page.url.host);
	const session = $derived(
		appSession.state.kind === 'signed-in' && appSession.state.reauth === undefined
			? appSession.state.session
			: undefined
	);
	const needsSignIn = $derived(phase === 'ready' && linkPresent && !session);

	async function loadTokenFromLocation() {
		secret = takeLinkSecret(location, history);
		linkPresent = secret !== undefined;
		errorMessage = undefined;
		deciding = undefined;
		if (!linkPresent) {
			phase = 'expired';
			return;
		}
		phase = 'loading';
		await appSession.boot();
		phase = 'ready';
		if (appSession.state.kind !== 'signed-in') {
			await tick();
			loginInput?.focus();
		}
	}

	onMount(() => {
		void loadTokenFromLocation();
	});

	onDestroy(() => {
		secret = undefined;
		password = '';
	});

	function onHashChange() {
		if (location.hash.length > 1) {
			void loadTokenFromLocation();
		}
	}

	async function submitSignIn(event: SubmitEvent) {
		event.preventDefault();
		if (signingIn) return;
		signInFailure = undefined;
		loginError = passwordError = undefined;
		const problems = checkSignIn(login, password);
		if (problems.length > 0) {
			for (const p of problems) {
				if (p.field === 'login') loginError = credentialCopy(p.field, p.rule);
				else passwordError = credentialCopy(p.field, p.rule);
			}
			(problems[0].field === 'login' ? loginInput : passwordInput)?.focus();
			return;
		}
		signingIn = true;
		try {
			await appSession.controller().signIn({ login, password });
			password = '';
			errorMessage = undefined;
		} catch (error) {
			signInFailure = describeCredentialFailure(error, Date.now());
			if (signInFailure.kind === 'mismatch') {
				password = '';
				await tick();
				passwordInput?.focus();
			}
		} finally {
			signingIn = false;
		}
	}

	async function decide(approve: boolean) {
		if (deciding || !secret) return;
		deciding = approve ? 'approve' : 'decline';
		errorMessage = undefined;
		try {
			const decision = await appSession.controller().decideCliApproval({ link: secret, approve });
			secret = undefined;
			if (decision.status === 'approved') phase = 'approved';
			else if (decision.status === 'declined') phase = 'declined';
			else phase = 'unknown';
		} catch (error) {
			const described = describeCliApprovalFailure(error);
			if (described.kind === 'link_used') {
				secret = undefined;
				linkPresent = false;
				phase = 'expired';
			} else {
				errorMessage = described.message;
			}
		} finally {
			deciding = undefined;
		}
	}
</script>

<svelte:window onhashchange={onHashChange} />
<svelte:head><title>Approve terminal sign-in · Agentic Org</title></svelte:head>

{#if phase === 'loading'}
	<p class="text-sm/6 text-fg-muted" aria-live="polite">Loading…</p>
{:else if phase === 'expired' || !linkPresent}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">{LINK_USED_COPY}</h1>
	<p class="mt-3 text-sm/6 text-fg-secondary">
		Start <code class="font-mono text-fg-bright">agentic-org sign-in</code> again in your terminal to
		get a new approval link.
	</p>
	<a
		href={resolve(session ? '/welcome' : '/sign-in')}
		class="mt-6 inline-block text-sm/6 font-semibold text-link hover:text-brand-text"
		>{session ? 'Back to organizations' : 'Sign in'}</a
	>
{:else if phase === 'approved'}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">Terminal sign-in approved</h1>
	<p class="mt-3 text-sm/6 text-fg-secondary" role="status">
		Your terminal is now signed in to HQ at {address}. You can return to your terminal and close
		this tab.
	</p>
	<a
		href={resolve('/welcome')}
		class="mt-6 inline-block text-sm/6 font-semibold text-link hover:text-brand-text"
		>Open organizations</a
	>
{:else if phase === 'declined'}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">Terminal sign-in declined</h1>
	<p class="mt-3 text-sm/6 text-fg-secondary" role="status">
		Nothing was signed in. You can close this tab.
	</p>
	<a
		href={resolve('/welcome')}
		class="mt-6 inline-block text-sm/6 font-semibold text-link hover:text-brand-text"
		>Open organizations</a
	>
{:else if phase === 'unknown'}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">Approval status unclear</h1>
	<p class="mt-3 text-sm/6 text-fg-secondary" role="alert">
		HQ returned an unrecognized response. Check your terminal before trying again.
	</p>
	<a
		href={resolve('/welcome')}
		class="mt-6 inline-block text-sm/6 font-semibold text-link hover:text-brand-text"
		>Open organizations</a
	>
{:else if needsSignIn}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">
		Sign in to approve terminal access
	</h1>
	<p class="mt-2 text-sm/6 text-fg-secondary">
		Sign in to HQ at {address} first, then choose whether to approve the terminal sign-in.
	</p>

	{#if signInFailure}
		<div
			role="alert"
			class="mt-6 border-l-4 border-danger bg-danger-emphasis/10 p-3 text-sm/6 text-danger-text-strong"
		>
			<p>{signInFailure.message}</p>
		</div>
	{/if}

	<form class="mt-6 flex flex-col gap-5" novalidate onsubmit={submitSignIn}>
		<div class="flex flex-col gap-1.5">
			<label for="cli-approve-login" class="block text-sm/6 font-medium text-fg">Username</label>
			<input
				id="cli-approve-login"
				bind:this={loginInput}
				bind:value={login}
				autocomplete="username"
				autocapitalize="none"
				spellcheck="false"
				readonly={signingIn}
				aria-invalid={loginError ? 'true' : undefined}
				aria-describedby={loginError ? 'cli-approve-login-error' : undefined}
				class="block w-full rounded-md bg-surface-raised px-3 py-1.5 text-base text-fg outline-1 -outline-offset-1 outline-line focus:outline-2 focus:-outline-offset-2 focus:outline-ring sm:text-sm/6"
			/>
			{#if loginError}
				<p id="cli-approve-login-error" class="text-sm/6 text-danger" role="alert">{loginError}</p>
			{/if}
		</div>
		<PasswordField
			id="cli-approve-password"
			label="Password"
			autocomplete="current-password"
			bind:value={password}
			bind:input={passwordInput}
			error={passwordError}
			readonly={signingIn}
		/>
		<div>
			<Button type="submit" disabled={signingIn}>
				{signingIn ? 'Signing in…' : 'Sign in to continue'}
			</Button>
		</div>
	</form>
{:else if session}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">Approve terminal sign-in</h1>
	<p class="mt-2 text-sm/6 text-fg-secondary">
		A terminal requested access to HQ at <span class="font-semibold text-fg">{address}</span>
		{#if appSession.authEnabled}
			as <span class="font-semibold text-fg">{session.account.login}</span>.
		{:else}
			in local mode.
		{/if}
	</p>

	{#if errorMessage}
		<div
			role="alert"
			class="mt-6 border-l-4 border-danger bg-danger-emphasis/10 p-3 text-sm/6 text-danger-text-strong"
		>
			<p>{errorMessage}</p>
		</div>
	{/if}

	<div class="mt-6 flex flex-wrap items-center gap-3">
		<Button type="button" disabled={deciding !== undefined} onclick={() => decide(true)}>
			{deciding === 'approve' ? 'Approving…' : 'Approve terminal sign-in'}
		</Button>
		<Button
			type="button"
			variant="outline"
			disabled={deciding !== undefined}
			onclick={() => decide(false)}
		>
			{deciding === 'decline' ? 'Declining…' : 'Decline'}
		</Button>
	</div>
{/if}
