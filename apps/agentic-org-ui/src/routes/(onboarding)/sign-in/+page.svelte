<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { onDestroy, onMount, tick } from 'svelte';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import {
		credentialCopy,
		describeCredentialFailure,
		rateLimitCopy,
		rateLimitExpired,
		type CredentialFailure
	} from '$lib/platform/auth/credential-copy';
	import { checkSignIn } from '$lib/platform/auth/credential-rules';
	import PasswordField from '$lib/platform/auth/PasswordField.svelte';
	import { safeReturnTarget } from '$lib/platform/auth/return-target';

	let login = $state('');
	let password = $state('');
	let loginError = $state<string>();
	let passwordError = $state<string>();
	let failure = $state<CredentialFailure>();
	let busy = $state(false);
	let retryUntil = $state<number>();
	let now = $state(Date.now());
	let loginInput = $state<HTMLInputElement | null>(null);
	let passwordInput = $state<HTMLInputElement | null>(null);
	let timer: ReturnType<typeof setInterval> | undefined;

	const address = $derived(page.url.host);
	const note = $derived(
		page.url.searchParams.get('note') === 'signed-out'
			? 'You signed out.'
			: page.url.searchParams.get('note') === 'ended'
				? 'Your session ended. Sign in again.'
				: undefined
	);
	const limited = $derived(retryUntil !== undefined && now < retryUntil);
	const failureText = $derived(
		failure?.kind === 'rate_limited' && retryUntil !== undefined
			? rateLimitCopy(retryUntil - now)
			: failure?.message
	);
	const ready = $derived(appSession.state.kind !== 'loading');
	// SIGN-31: this tab's account was replaced by a sign-in in another tab.
	const replacedBy = $derived(
		appSession.state.kind === 'signed-out' && appSession.state.reason === 'changed-account'
			? appSession.state.login
			: undefined
	);

	function destination() {
		return safeReturnTarget(page.url.searchParams.get('next'), page.url.origin) ?? '/welcome';
	}

	/**
	 * Enters HQ after a sign-in. A navigation failure is not a sign-in failure: the person is
	 * signed in, so a target that cannot be opened falls back to /welcome (SIGN-8), never to the
	 * sign-in error copy.
	 */
	async function enter() {
		try {
			// resolve() reads a query as route segments: an unencoded '/', '(' or '[' in it would
			// change or reject the target (/chat?thread=a//b). The query is rebuilt with
			// URLSearchParams, which encodes them; every parameter keeps its value.
			const target = destination();
			const query = target.indexOf('?');
			const path = query < 0 ? target : target.slice(0, query);
			const search = query < 0 ? '' : new URLSearchParams(target.slice(query)).toString();
			await goto(resolve((search ? `${path}?${search}` : path) as '/'), {
				replaceState: true
			});
		} catch {
			await goto(resolve('/welcome'), { replaceState: true }).catch(() =>
				location.assign(resolve('/welcome'))
			);
		}
	}

	onMount(async () => {
		await appSession.boot();
		if (appSession.state.kind === 'signed-in' && appSession.state.reauth === undefined) {
			await enter();
			return;
		}
		await tick();
		(login ? passwordInput : loginInput)?.focus();
	});
	onDestroy(() => clearInterval(timer));

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy || limited) return;
		failure = undefined;
		loginError = passwordError = undefined;
		const problems = checkSignIn(login, password);
		if (problems.length > 0) {
			// SIGN-3: no request; message under the field; focus the first empty field.
			for (const p of problems) {
				if (p.field === 'login') loginError = credentialCopy(p.field, p.rule);
				else passwordError = credentialCopy(p.field, p.rule);
			}
			(problems[0].field === 'login' ? loginInput : passwordInput)?.focus();
			return;
		}
		busy = true;
		try {
			let signedIn = false;
			try {
				await appSession.controller().signIn({ login, password });
				signedIn = true;
			} catch (error) {
				failure = describeCredentialFailure(error, Date.now());
				if (failure.kind === 'mismatch') {
					// SIGN-4: Password cleared, Username kept, focus to Password.
					password = '';
					await tick();
					passwordInput?.focus();
				}
				if (failure.kind === 'rate_limited' && failure.retryUntilMs !== undefined) {
					retryUntil = failure.retryUntilMs;
					now = Date.now();
					clearInterval(timer);
					timer = setInterval(() => {
						now = Date.now();
						// SIGN-6: when the retry time passes, the message ends with the countdown.
						if (retryUntil === undefined || now >= retryUntil) {
							if (rateLimitExpired(failure, now)) failure = undefined;
							retryUntil = undefined;
							clearInterval(timer);
						}
					}, 1_000);
				}
			}
			if (signedIn) {
				password = '';
				await enter();
			}
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>Sign in · Agentic Org</title></svelte:head>

{#if !ready}
	<p class="text-sm/6 text-fg-muted" aria-live="polite">Loading…</p>
{:else}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">Sign in to HQ at {address}</h1>
	{#if replacedBy !== undefined}
		<div role="status" class="mt-2 text-sm/6 text-fg-secondary">
			<p>
				You're now signed in as {replacedBy} in another tab. Reload to continue as {replacedBy}.
			</p>
			<button type="button" class="mt-1 font-semibold underline" onclick={() => location.reload()}
				>Reload</button
			>
		</div>
	{:else if note}<p class="mt-2 text-sm/6 text-fg-secondary" role="status">{note}</p>{/if}

	<div aria-live="assertive">
		{#if failure && failureText}
			<div
				id="sign-in-error"
				role="alert"
				class="mt-6 border-l-4 border-danger bg-danger-emphasis/10 p-3 text-sm/6 text-danger-text-strong"
			>
				<p>{failureText}</p>
				{#if failure.kind === 'unreachable' || failure.kind === 'hq_error'}
					<button type="submit" form="sign-in-form" class="mt-2 font-semibold underline"
						>Try again</button
					>
				{:else if failure.kind === 'out_of_date'}
					<button
						type="button"
						class="mt-2 font-semibold underline"
						onclick={() => location.reload()}>Reload</button
					>
				{/if}
			</div>
		{/if}
	</div>

	<form
		id="sign-in-form"
		class="mt-6 space-y-6"
		novalidate
		onsubmit={submit}
		aria-describedby={failure ? 'sign-in-error' : undefined}
	>
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
		<PasswordField
			id="password"
			label="Password"
			autocomplete="current-password"
			bind:value={password}
			bind:input={passwordInput}
			error={passwordError}
			readonly={busy}
		/>
		<button
			type="submit"
			disabled={busy || limited}
			class="flex w-full justify-center rounded-md bg-primary px-3 py-1.5 text-sm/6 font-semibold text-on-emphasis hover:bg-primary-hover disabled:opacity-60 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
			>{busy ? 'Signing in…' : 'Sign in'}</button
		>
	</form>

	<div class="mt-8 space-y-2 text-sm/6 text-fg-muted">
		<p>
			Forgot your password? Ask the owner of your organization for a new sign-in link. The
			administrator, at the HQ machine, runs:
		</p>
		<p><code class="text-fg-body">agentic-org admin reset --name &lt;instance&gt;</code></p>
		<p>
			(on Linux, with <code class="text-fg-body">sudo</code> in front:
			<code class="text-fg-body">sudo agentic-org admin reset --name &lt;instance&gt;</code>).
		</p>
		<p>Need an account? Ask the owner of your organization to invite you.</p>
	</div>
{/if}
