<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { onDestroy, onMount, tick } from 'svelte';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import { credentialCopy, describeCredentialFailure, rateLimitCopy, type CredentialFailure } from '$lib/platform/auth/credential-copy';
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
		failure?.kind === 'rate_limited' && retryUntil !== undefined ? rateLimitCopy(retryUntil - now) : failure?.message
	);
	const ready = $derived(appSession.state.kind !== 'loading');
	// SIGN-31: this tab's account was replaced by a sign-in in another tab.
	const replacedBy = $derived(
		appSession.state.kind === 'signed-out' && appSession.state.reason === 'changed-account' ? appSession.state.login : undefined
	);

	function destination() {
		return safeReturnTarget(page.url.searchParams.get('next'), page.url.origin) ?? '/welcome';
	}

	onMount(async () => {
		await appSession.boot();
		if (appSession.state.kind === 'signed-in' && appSession.state.reauth === undefined) {
			await goto(destination(), { replaceState: true });
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
			await appSession.controller().signIn({ login, password });
			password = '';
			await goto(destination(), { replaceState: true });
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
					if (retryUntil !== undefined && now >= retryUntil) clearInterval(timer);
				}, 1_000);
			}
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>Sign in · Agentic Org</title></svelte:head>

{#if !ready}
	<p class="text-sm/6 text-gray-400" aria-live="polite">Loading…</p>
{:else}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-white">Sign in to HQ at {address}</h1>
	{#if replacedBy !== undefined}
		<div role="status" class="mt-2 text-sm/6 text-gray-300">
			<p>You're now signed in as {replacedBy} in another tab. Reload to continue as {replacedBy}.</p>
			<button type="button" class="mt-1 font-semibold underline" onclick={() => location.reload()}>Reload</button>
		</div>
	{:else if note}<p class="mt-2 text-sm/6 text-gray-300" role="status">{note}</p>{/if}

	<div aria-live="assertive">
		{#if failure && failureText}
			<div id="sign-in-error" role="alert" class="mt-6 border-l-4 border-red-400 bg-red-500/10 p-3 text-sm/6 text-red-200">
				<p>{failureText}</p>
				{#if failure.kind === 'unreachable' || failure.kind === 'hq_error'}
					<button type="submit" form="sign-in-form" class="mt-2 font-semibold underline">Try again</button>
				{:else if failure.kind === 'out_of_date'}
					<button type="button" class="mt-2 font-semibold underline" onclick={() => location.reload()}>Reload</button>
				{/if}
			</div>
		{/if}
	</div>

	<form id="sign-in-form" class="mt-6 space-y-6" novalidate onsubmit={submit} aria-describedby={failure ? 'sign-in-error' : undefined}>
		<div>
			<label for="login" class="block text-sm/6 font-medium text-white">Username</label>
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
				class="mt-2 block w-full rounded-md bg-white/5 px-3 py-1.5 text-base text-white outline-1 -outline-offset-1 outline-white/10 focus:outline-2 focus:-outline-offset-2 focus:outline-indigo-500 sm:text-sm/6"
			/>
			{#if loginError}<p id="login-error" class="mt-2 text-sm/6 text-red-400">{loginError}</p>{/if}
		</div>
		<PasswordField id="password" label="Password" autocomplete="current-password" bind:value={password} bind:input={passwordInput} error={passwordError} readonly={busy} />
		<button
			type="submit"
			disabled={busy || limited}
			class="flex w-full justify-center rounded-md bg-indigo-500 px-3 py-1.5 text-sm/6 font-semibold text-white hover:bg-indigo-600 disabled:opacity-60 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-500"
			>{busy ? 'Signing in…' : 'Sign in'}</button
		>
	</form>

	<div class="mt-8 space-y-2 text-sm/6 text-gray-400">
		<p>Forgot your password? Ask the owner of your organization for a new sign-in link. The administrator, at the HQ machine, runs:</p>
		<p><code class="text-gray-200">agentic-org admin reset --name &lt;instance&gt;</code></p>
		<p>(on Linux, with <code class="text-gray-200">sudo</code> in front: <code class="text-gray-200">sudo agentic-org admin reset --name &lt;instance&gt;</code>).</p>
		<p>Need an account? Ask the owner of your organization to invite you.</p>
	</div>
{/if}
