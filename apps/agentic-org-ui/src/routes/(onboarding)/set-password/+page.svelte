<script lang="ts">
	import { resolve } from '$app/paths';
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
	import { SessionRequestError } from '$lib/platform/auth/session-gateway';

	let secret: string | undefined;
	let linkPresent = $state(true);
	let login = $state<string>();
	let password = $state('');
	let again = $state('');
	let passwordError = $state<string>();
	let failure = $state<CredentialFailure>();
	let busy = $state(false);
	let doneLogin = $state<string>();
	let passwordInput = $state<HTMLInputElement | null>(null);

	onMount(async () => {
		secret = takeLinkSecret(location, history);
		linkPresent = secret !== undefined;
		if (secret) {
			try {
				const info = await appSession
					.controller()
					.inspectLink({ link: secret, kind: 'set_password' });
				if (info.kind === 'set_password') {
					login = info.login;
				}
			} catch (error) {
				if (
					error instanceof SessionRequestError &&
					(error.status === 410 || error.status === 404)
				) {
					secret = undefined;
					linkPresent = false;
				}
			}
		}
		await tick();
		passwordInput?.focus();
	});

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy || !secret) return;
		failure = undefined;
		passwordError = undefined;
		const problems = checkRegistration(login ?? 'placeholder', password, again).filter(
			(p) => p.field === 'password'
		);
		if (problems.some((p) => p.rule === 'mismatch')) password = again = '';
		if (problems.length > 0) {
			passwordError = credentialCopy('password', problems[0].rule, 'register');
			passwordInput?.focus();
			return;
		}
		busy = true;
		try {
			const res = await appSession.controller().setPassword({ link: secret, password });
			secret = undefined;
			password = again = '';
			doneLogin = res.login;
		} catch (error) {
			const result = describeCredentialFailure(error, Date.now(), 'register');
			if (result.kind === 'field') {
				passwordError = result.message;
				await tick();
				passwordInput?.focus();
			} else if (result.kind === 'link_used') {
				secret = undefined;
				linkPresent = false;
			} else {
				failure = result;
			}
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>Set a new password · Agentic Org</title></svelte:head>

{#if doneLogin}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">Password updated</h1>
	<p class="mt-3 text-sm/6 text-fg-secondary">
		Your password for <span class="font-mono text-fg">{doneLogin}</span> has been updated.
	</p>
	<a
		href={resolve('/sign-in')}
		class="mt-6 inline-flex rounded-md bg-primary px-3 py-1.5 text-sm/6 font-semibold text-on-emphasis hover:bg-primary-hover"
		>Sign in</a
	>
{:else if !linkPresent}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">{LINK_USED_COPY}</h1>
	<p class="mt-4 text-sm/6 text-fg-secondary">
		Ask the owner of your organization or the instance administrator for a new sign-in link.
	</p>
	<a
		href={resolve('/sign-in')}
		class="mt-6 inline-block text-sm/6 font-semibold text-link hover:text-brand-text">Sign in</a
	>
{:else}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">Set a new password</h1>
	{#if login}
		<p class="mt-2 text-sm/6 text-fg-muted">
			Choose a new password for <span class="font-mono text-fg">{login}</span>.
		</p>
	{/if}

	<div aria-live="assertive">
		{#if failure}
			<div
				role="alert"
				class="mt-6 border-l-4 border-danger bg-danger-emphasis/10 p-3 text-sm/6 text-danger-text-strong"
			>
				<p>{failure.message}</p>
			</div>
		{/if}
	</div>

	<form class="mt-6 space-y-6" novalidate onsubmit={submit}>
		<PasswordField
			id="new-password"
			label="New password (at least 12 characters)"
			autocomplete="new-password"
			bind:value={password}
			bind:input={passwordInput}
			error={passwordError}
			readonly={busy}
		/>
		<PasswordField
			id="new-password-again"
			label="New password again"
			autocomplete="new-password"
			bind:value={again}
			readonly={busy}
		/>
		<button
			type="submit"
			disabled={busy}
			class="flex w-full justify-center rounded-md bg-primary px-3 py-1.5 text-sm/6 font-semibold text-on-emphasis hover:bg-primary-hover disabled:opacity-60 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
			>{busy ? 'Updating password…' : 'Set new password'}</button
		>
	</form>
{/if}
