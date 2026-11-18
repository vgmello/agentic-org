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
	import { checkRegistration } from '$lib/platform/auth/credential-rules';
	import { takeLinkSecret } from '$lib/platform/auth/link-fragment';
	import PasswordField from '$lib/platform/auth/PasswordField.svelte';

	// Held in this closure only after mount: never in reactive state, storage, a request or a log.
	// The address bar and history entry lose the fragment before any request (takeLinkSecret).
	// SvelteKit's `page.url` keeps the URL this page view loaded with, fragment included, until the
	// next navigation: shallow routing never rewrites it. This page reads only `page.url.host`.
	let secret: string | undefined;
	let linkPresent = $state(true);
	let login = $state('');
	let password = $state('');
	let again = $state('');
	let loginError = $state<string>();
	let passwordError = $state<string>();
	let failure = $state<CredentialFailure>();
	let busy = $state(false);
	let loginInput = $state<HTMLInputElement | null>(null);
	let passwordInput = $state<HTMLInputElement | null>(null);
	const address = $derived(page.url.host);

	onMount(async () => {
		secret = takeLinkSecret(location, history);
		linkPresent = secret !== undefined;
		await tick();
		loginInput?.focus();
	});

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy || !secret) return;
		failure = undefined;
		loginError = passwordError = undefined;
		const problems = checkRegistration(login, password, again);
		for (const p of problems) {
			if (p.field === 'login') loginError = credentialCopy(p.field, p.rule, 'register');
			else passwordError = credentialCopy(p.field, p.rule, 'register');
		}
		if (problems.some((p) => p.rule === 'mismatch')) password = again = '';
		if (problems.length > 0) {
			(problems[0].field === 'login' ? loginInput : passwordInput)?.focus();
			return;
		}
		busy = true;
		try {
			await appSession.controller().register({ link: secret, login, password });
			secret = undefined;
			password = again = '';
			await goto(resolve('/welcome'), { replaceState: true });
		} catch (error) {
			const result = describeCredentialFailure(error, Date.now(), 'register');
			if (result.kind === 'field') {
				if (result.field === 'login') loginError = result.message;
				else passwordError = result.message;
				await tick();
				(result.field === 'login' ? loginInput : passwordInput)?.focus();
			} else if (result.kind === 'link_used') {
				secret = undefined;
				linkPresent = false;
			} else failure = result;
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>Create your account · Agentic Org</title></svelte:head>

{#if !linkPresent}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-white">{LINK_USED_COPY}</h1>
	<p class="mt-4 text-sm/6 text-gray-300">
		For the administrator: on the HQ machine, run <code class="text-gray-100"
			>agentic-org admin reset --name &lt;instance&gt;</code
		>.
	</p>
	<p class="mt-2 text-sm/6 text-gray-300">
		For anyone else: ask the owner of your organization for a new link.
	</p>
	<a
		href={resolve('/sign-in')}
		class="mt-6 inline-block text-sm/6 font-semibold text-indigo-400 hover:text-indigo-300"
		>Sign in</a
	>
{:else}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-white">
		Create your account on HQ at {address}
	</h1>
	<p class="mt-2 text-sm/6 text-gray-400">It will be this instance's administrator.</p>

	<div aria-live="assertive">
		{#if failure}
			<div
				id="register-error"
				role="alert"
				class="mt-6 border-l-4 border-red-400 bg-red-500/10 p-3 text-sm/6 text-red-200"
			>
				<p>{failure.message}</p>
				{#if failure.kind === 'out_of_date'}
					<button
						type="button"
						class="mt-2 font-semibold underline"
						onclick={() => location.reload()}>Reload</button
					>
				{:else}
					<button type="submit" form="register-form" class="mt-2 font-semibold underline"
						>Try again</button
					>
				{/if}
			</div>
		{/if}
	</div>

	<form id="register-form" class="mt-6 space-y-6" novalidate onsubmit={submit}>
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
		<button
			type="submit"
			disabled={busy}
			class="flex w-full justify-center rounded-md bg-indigo-500 px-3 py-1.5 text-sm/6 font-semibold text-white hover:bg-indigo-600 disabled:opacity-60 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-500"
			>{busy ? 'Creating account…' : 'Create account'}</button
		>
	</form>
{/if}
