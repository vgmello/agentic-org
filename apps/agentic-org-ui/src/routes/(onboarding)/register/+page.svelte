<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { onMount, tick } from 'svelte';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import {
		credentialCopy,
		describeCredentialFailure,
		LINK_USED_COPY,
		type CredentialFailure
	} from '$lib/platform/auth/credential-copy';
	import { checkRegistration, checkSignIn } from '$lib/platform/auth/credential-rules';
	import { takeLinkSecret } from '$lib/platform/auth/link-fragment';
	import PasswordField from '$lib/platform/auth/PasswordField.svelte';
	import { SessionRequestError } from '$lib/platform/auth/session-gateway';

	type PageFailure = CredentialFailure | { kind: 'conflict'; message: string };
	const LINK_CONFLICT_COPY = 'This account is already linked to someone in this organization.';

	// Held in this closure only after mount: never in reactive state, storage, a request or a log.
	// The address bar and history entry lose the fragment before any request (takeLinkSecret).
	let secret: string | undefined;
	let inspecting = $state(true);
	let linkPresent = $state(true);
	let inviteOrgName = $state<string>();
	let mode = $state<'create' | 'sign-in'>('create');
	let login = $state('');
	let password = $state('');
	let again = $state('');
	let loginError = $state<string>();
	let passwordError = $state<string>();
	let failure = $state<PageFailure>();
	let busy = $state(false);
	let loginInput = $state<HTMLInputElement | null>(null);
	let passwordInput = $state<HTMLInputElement | null>(null);
	const address = $derived(page.url.host);

	async function inspectCurrentLink(token: string) {
		try {
			const info = await appSession.controller().inspectLink({ link: token, kind: 'registration' });
			if (info.kind === 'organization_invite') inviteOrgName = info.org_name;
		} catch (error) {
			if (error instanceof SessionRequestError && (error.status === 404 || error.status === 410)) {
				secret = undefined;
				linkPresent = false;
			}
		}
	}

	onMount(async () => {
		secret = takeLinkSecret(location, history);
		linkPresent = secret !== undefined;
		if (secret) await inspectCurrentLink(secret);
		inspecting = false;
		await tick();
		loginInput?.focus();
	});

	function switchMode(next: 'create' | 'sign-in') {
		mode = next;
		loginError = passwordError = undefined;
		failure = undefined;
		password = again = '';
	}

	function validateForm(isAttach: boolean): boolean {
		const ctx = isAttach ? 'sign-in' : 'register';
		const problems = isAttach
			? checkSignIn(login, password)
			: checkRegistration(login, password, again);
		for (const p of problems) {
			if (p.field === 'login') loginError = credentialCopy(p.field, p.rule, ctx);
			else passwordError = credentialCopy(p.field, p.rule, ctx);
		}
		if (problems.some((p) => p.rule === 'mismatch')) password = again = '';
		if (problems.length === 0) return true;
		(problems[0].field === 'login' ? loginInput : passwordInput)?.focus();
		return false;
	}

	async function handleSubmitError(error: unknown, isAttach: boolean) {
		if (error instanceof SessionRequestError) {
			if (error.status === 404 || error.status === 410) {
				secret = undefined;
				linkPresent = false;
				return;
			}
			if (error.status === 409) {
				failure = { kind: 'conflict', message: LINK_CONFLICT_COPY };
				return;
			}
		}
		const result = describeCredentialFailure(error, Date.now(), isAttach ? 'sign-in' : 'register');
		if (result.kind === 'field') {
			if (result.field === 'login') loginError = result.message;
			else passwordError = result.message;
			await tick();
			(result.field === 'login' ? loginInput : passwordInput)?.focus();
			return;
		}
		if (result.kind === 'link_used') {
			secret = undefined;
			linkPresent = false;
			return;
		}
		failure = result;
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy || !secret) return;
		failure = undefined;
		loginError = passwordError = undefined;
		const isAttach = Boolean(inviteOrgName) && mode === 'sign-in';
		if (!validateForm(isAttach)) return;
		busy = true;
		try {
			const payload = { link: secret, login, password };
			if (isAttach) await appSession.controller().registerSignIn(payload);
			else await appSession.controller().register(payload);
			secret = undefined;
			password = again = '';
			await goto(resolve('/welcome'), { replaceState: true });
		} catch (error) {
			await handleSubmitError(error, isAttach);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head>
	<title>{inviteOrgName ? `Join ${inviteOrgName}` : 'Create your account'} · Agentic Org</title>
</svelte:head>

{#if inspecting}
	<p class="text-sm/6 text-fg-muted" role="status">Checking link…</p>
{:else if !linkPresent}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">{LINK_USED_COPY}</h1>
	<p class="mt-4 text-sm/6 text-fg-secondary">
		For the administrator: on the HQ machine, run <code class="text-fg-bright"
			>agentic-org admin reset --name &lt;instance&gt;</code
		>.
	</p>
	<p class="mt-2 text-sm/6 text-fg-secondary">
		For anyone else: ask the owner of your organization for a new link.
	</p>
	<a
		href={resolve('/sign-in')}
		class="mt-6 inline-block text-sm/6 font-semibold text-link hover:text-brand-text">Sign in</a
	>
{:else}
	{#if inviteOrgName}
		<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">
			Join {inviteOrgName} on HQ at {address}
		</h1>
		<p class="mt-2 text-sm/6 text-fg-muted">
			{mode === 'create'
				? 'Create an account to sign in, or attach an account you already have on this instance.'
				: 'Sign in with your existing account on this instance to accept this invitation.'}
		</p>
		<div class="mt-4 flex gap-x-2" role="group" aria-label="Invitation method">
			<button
				type="button"
				disabled={busy}
				onclick={() => switchMode('create')}
				class="rounded-md px-3 py-1.5 text-sm/6 font-semibold {mode === 'create'
					? 'bg-primary text-on-emphasis'
					: 'bg-surface-raised text-fg outline-1 -outline-offset-1 outline-line hover:bg-surface-overlay'}"
				>Create new account</button
			>
			<button
				type="button"
				disabled={busy}
				onclick={() => switchMode('sign-in')}
				class="rounded-md px-3 py-1.5 text-sm/6 font-semibold {mode === 'sign-in'
					? 'bg-primary text-on-emphasis'
					: 'bg-surface-raised text-fg outline-1 -outline-offset-1 outline-line hover:bg-surface-overlay'}"
				>Use existing account</button
			>
		</div>
	{:else}
		<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">
			Create your account on HQ at {address}
		</h1>
		<p class="mt-2 text-sm/6 text-fg-muted">It will be this instance's administrator.</p>
	{/if}

	<div aria-live="assertive">
		{#if failure}
			<div
				id="register-error"
				role="alert"
				class="mt-6 border-l-4 border-danger bg-danger-emphasis/10 p-3 text-sm/6 text-danger-text-strong"
			>
				<p>{failure.message}</p>
				{#if failure.kind === 'out_of_date'}
					<button
						type="button"
						class="mt-2 font-semibold underline"
						onclick={() => location.reload()}>Reload</button
					>
				{:else if failure.kind !== 'conflict'}
					<button type="submit" form="register-form" class="mt-2 font-semibold underline"
						>Try again</button
					>
				{/if}
			</div>
		{/if}
	</div>

	<form id="register-form" class="mt-6 space-y-6" novalidate onsubmit={submit}>
		<div>
			<label for="login" class="block text-sm/6 font-medium text-fg">Username</label>
			<input
				id="login"
				bind:this={loginInput}
				bind:value={login}
				autocomplete="username"
				autocapitalize="none"
				spellcheck="false"
				readonly={busy}
				aria-invalid={loginError ? 'true' : undefined}
				aria-describedby={loginError ? 'login-error' : undefined}
				class="mt-2 block w-full rounded-md bg-surface-raised px-3 py-1.5 text-base text-fg outline-1 -outline-offset-1 outline-line focus:outline-2 focus:-outline-offset-2 focus:outline-ring sm:text-sm/6"
			/>
			{#if loginError}<p id="login-error" class="mt-2 text-sm/6 text-danger">{loginError}</p>{/if}
		</div>
		{#if inviteOrgName && mode === 'sign-in'}
			<PasswordField
				id="password"
				label="Password"
				autocomplete="current-password"
				bind:value={password}
				bind:input={passwordInput}
				error={passwordError}
				readonly={busy}
			/>
		{:else}
			<PasswordField
				id="password"
				label="Password (at least 12 characters)"
				autocomplete="new-password"
				bind:value={password}
				bind:input={passwordInput}
				error={passwordError}
				readonly={busy}
			/>
			<PasswordField
				id="password-again"
				label="Password again"
				autocomplete="new-password"
				bind:value={again}
				readonly={busy}
			/>
		{/if}
		<button
			type="submit"
			disabled={busy}
			class="flex w-full justify-center rounded-md bg-primary px-3 py-1.5 text-sm/6 font-semibold text-on-emphasis hover:bg-primary-hover disabled:opacity-60 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
			>{busy
				? inviteOrgName && mode === 'sign-in'
					? 'Signing in…'
					: 'Creating account…'
				: inviteOrgName && mode === 'sign-in'
					? 'Sign in and join'
					: 'Create account'}</button
		>
	</form>
{/if}
