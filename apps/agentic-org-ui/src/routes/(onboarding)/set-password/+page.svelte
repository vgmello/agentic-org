<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { onDestroy, onMount, tick } from 'svelte';
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
	let inspecting = $state(true);
	let linkPresent = $state(true);
	let login = $state<string>();
	let password = $state('');
	let again = $state('');
	let passwordError = $state<string>();
	let failure = $state<CredentialFailure>();
	let busy = $state(false);
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
		inspecting = false;
		await tick();
		passwordInput?.focus();
	});

	onDestroy(() => {
		secret = undefined;
		password = '';
		again = '';
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
			await tick();
			passwordInput?.focus();
			return;
		}
		busy = true;
		try {
			const res = await appSession.controller().setPassword({ link: secret, password });
			secret = undefined;
			password = again = '';
			const query = new URLSearchParams({ note: 'password-set', login: res.login }).toString();
			await goto(resolve(`/sign-in?${query}` as '/sign-in'), { replaceState: true });
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
	<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">
		{#if login}
			Set a new password for <span class="font-mono">{login}</span>
		{:else}
			Set a new password
		{/if}
	</h1>
	<p class="mt-2 text-sm/6 text-fg-muted">
		Setting a new password signs this account out in every browser.
	</p>

	<div aria-live="assertive">
		{#if failure}
			<div
				id="set-password-error"
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
				{:else if failure.kind === 'unreachable' || failure.kind === 'hq_error'}
					<button type="submit" form="set-password-form" class="mt-2 font-semibold underline"
						>Try again</button
					>
				{/if}
			</div>
		{/if}
	</div>

	<form
		id="set-password-form"
		class="mt-6 flex flex-col gap-6"
		novalidate
		onsubmit={submit}
		aria-describedby={failure ? 'set-password-error' : undefined}
	>
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
			label="Password again"
			autocomplete="new-password"
			bind:value={again}
			readonly={busy}
		/>
		<button
			type="submit"
			disabled={busy}
			class="flex w-full justify-center rounded-md bg-primary px-3 py-1.5 text-sm/6 font-semibold text-on-emphasis hover:bg-primary-hover disabled:opacity-60 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
			>{busy ? 'Setting password…' : 'Set password'}</button
		>
	</form>
{/if}
