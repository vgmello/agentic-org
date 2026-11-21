<script lang="ts">
	import { onMount, tick } from 'svelte';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import Lock from '@lucide/svelte/icons/lock';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import * as Alert from '$lib/components/ui/alert';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { tone } from '$lib/components/pills';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import PasswordField from '$lib/platform/auth/PasswordField.svelte';
	import { checkRegistration } from '$lib/platform/auth/credential-rules';
	import {
		credentialCopy,
		describeCredentialFailure,
		type CredentialFailure
	} from '$lib/platform/auth/credential-copy';
	import type { HstsMode, TlsEnableResult } from '$lib/platform/http/wire';
	import {
		describeTlsFailure,
		hstsLabel,
		isAuthAlreadyEnabled,
		listenSchemeLabel,
		validateTlsFields,
		type TlsMode
	} from './security-copy';

	let { standalone = false }: { standalone?: boolean } = $props();

	let authLogin = $state('');
	let authPassword = $state('');
	let authAgain = $state('');
	let authLoginError = $state<string>();
	let authPasswordError = $state<string>();
	let authFailure = $state<CredentialFailure>();
	let authNotice = $state<string>();
	let authBusy = $state(false);
	let authLoginInput = $state<HTMLInputElement | null>(null);
	let authPasswordInput = $state<HTMLInputElement | null>(null);

	let tlsMode = $state<TlsMode>('self_signed');
	let tlsCert = $state('');
	let tlsKey = $state('');
	let tlsHsts = $state<HstsMode>('off');
	let tlsFieldErrors = $state<{ cert?: string; key?: string }>({});
	let tlsError = $state<string>();
	let tlsResult = $state<TlsEnableResult>();
	let tlsBusy = $state(false);

	const session = $derived(
		appSession.state.kind === 'signed-in' ? appSession.state.session : undefined
	);
	const authEnabled = $derived(appSession.authEnabled);
	const listenScheme = $derived(appSession.listenScheme);
	const isHttps = $derived(listenScheme === 'https');
	const isAdmin = $derived(session?.admin ?? false);
	const tlsBlockedByAuth = $derived(!authEnabled);
	const tlsDisabled = $derived(tlsBusy || tlsBlockedByAuth || !isAdmin || !session);

	onMount(async () => {
		await appSession.boot();
		if (appSession.state.kind === 'signed-in') {
			await appSession
				.controller()
				.refreshAccess()
				.catch(() => undefined);
		}
	});

	async function handleEnableAuthError(error: unknown) {
		if (isAuthAlreadyEnabled(error)) {
			authPassword = '';
			authAgain = '';
			authNotice = 'Authentication is already enabled on this HQ.';
			return;
		}
		const described = describeCredentialFailure(error, Date.now(), 'register');
		if (described.kind !== 'field') {
			authFailure = described;
			return;
		}
		if (described.field === 'login') authLoginError = described.message;
		else authPasswordError = described.message;
		await tick();
		(described.field === 'login' ? authLoginInput : authPasswordInput)?.focus();
	}

	async function submitEnableAuth(event: SubmitEvent) {
		event.preventDefault();
		if (authBusy || authEnabled) return;
		authFailure = undefined;
		authNotice = undefined;
		authLoginError = authPasswordError = undefined;
		const problems = checkRegistration(authLogin, authPassword, authAgain);
		for (const p of problems) {
			if (p.field === 'login') authLoginError = credentialCopy(p.field, p.rule, 'register');
			else authPasswordError = credentialCopy(p.field, p.rule, 'register');
		}
		if (problems.some((p) => p.rule === 'mismatch')) authPassword = authAgain = '';
		if (problems.length > 0) {
			await tick();
			(problems[0].field === 'login' ? authLoginInput : authPasswordInput)?.focus();
			return;
		}
		authBusy = true;
		try {
			await appSession.controller().enableAuth({ login: authLogin, password: authPassword });
			authPassword = '';
			authAgain = '';
			authNotice = 'Authentication is now enabled. You remain signed in on this browser.';
		} catch (error) {
			await handleEnableAuthError(error);
		} finally {
			authBusy = false;
		}
	}

	async function submitEnableTls(event: SubmitEvent) {
		event.preventDefault();
		if (tlsDisabled) return;
		tlsError = undefined;
		tlsFieldErrors = {};
		const validation = validateTlsFields(tlsMode, tlsCert, tlsKey);
		if (validation) {
			tlsFieldErrors = validation;
			return;
		}
		tlsBusy = true;
		try {
			const payload =
				tlsMode === 'self_signed'
					? { mode: 'self_signed' as const, hsts: tlsHsts }
					: { mode: 'supplied' as const, cert: tlsCert, key: tlsKey, hsts: tlsHsts };
			const result = await appSession.controller().enableTls(payload);
			tlsResult = result;
			tlsCert = '';
			tlsKey = '';
		} catch (error) {
			tlsError = describeTlsFailure(error);
		} finally {
			tlsBusy = false;
		}
	}

	const cardClass =
		'rounded-lg bg-surface-raised p-6 outline-1 -outline-offset-1 outline-line flex flex-col gap-4';
	const headingTag = $derived(standalone ? 'h2' : 'h3');
</script>

<div class="flex flex-col gap-6" data-testid="security-access-panel">
	<section aria-labelledby="security-auth-heading" class={cardClass}>
		<div class="flex flex-wrap items-start justify-between gap-3">
			<div class="flex flex-col gap-1">
				<div class="flex items-center gap-2">
					<ShieldCheck class="size-4 text-fg-muted" aria-hidden="true" />
					<svelte:element
						this={headingTag}
						id="security-auth-heading"
						class="text-base/7 font-semibold text-fg"
					>
						Authentication
					</svelte:element>
				</div>
				<p class="text-sm/6 text-fg-muted">
					Control whether opening this HQ in a browser requires a username and password.
				</p>
			</div>
			<StatusPill
				label={authEnabled ? 'Enabled' : 'Disabled (local HTTP mode)'}
				class={authEnabled ? tone.ok : tone.owner}
			/>
		</div>

		{#if authNotice}
			<Alert.Root>
				<Alert.Title>Authentication active</Alert.Title>
				<Alert.Description>{authNotice}</Alert.Description>
			</Alert.Root>
		{/if}

		{#if authEnabled}
			<p class="text-sm/6 text-fg-secondary" data-testid="auth-enabled-summary">
				Password and session sign-in is active for this HQ instance.
				{#if session}
					Signed in as <span class="font-semibold text-fg">{session.account.login}</span>.
				{/if}
			</p>
		{:else}
			<p class="text-sm/6 text-fg-secondary">
				This local HQ started over plain HTTP without a sign-in wall. Create the administrator
				username and password below to require sign-in and unlock HTTPS.
			</p>

			{#if authFailure}
				<div
					role="alert"
					class="border-l-4 border-danger bg-danger-emphasis/10 p-3 text-sm/6 text-danger-text-strong"
				>
					<p>{authFailure.message}</p>
				</div>
			{/if}

			<form class="flex max-w-md flex-col gap-4" novalidate onsubmit={submitEnableAuth}>
				<div class="flex flex-col gap-1.5">
					<Label for="enable-auth-login">Username</Label>
					<input
						id="enable-auth-login"
						bind:this={authLoginInput}
						bind:value={authLogin}
						autocomplete="username"
						autocapitalize="none"
						spellcheck="false"
						readonly={authBusy}
						aria-invalid={authLoginError ? 'true' : undefined}
						aria-describedby={authLoginError ? 'enable-auth-login-error' : undefined}
						class="block w-full rounded-md bg-surface-sunken px-3 py-1.5 text-base text-fg outline-1 -outline-offset-1 outline-line focus:outline-2 focus:-outline-offset-2 focus:outline-ring sm:text-sm/6"
					/>
					{#if authLoginError}
						<p id="enable-auth-login-error" class="text-sm/6 text-danger" role="alert">
							{authLoginError}
						</p>
					{/if}
				</div>

				<PasswordField
					id="enable-auth-password"
					label="Password (at least 12 characters)"
					autocomplete="new-password"
					bind:value={authPassword}
					bind:input={authPasswordInput}
					error={authPasswordError}
					readonly={authBusy}
				/>

				<PasswordField
					id="enable-auth-password-again"
					label="Confirm password"
					autocomplete="new-password"
					bind:value={authAgain}
					readonly={authBusy}
				/>

				<div>
					<Button type="submit" disabled={authBusy}>
						{authBusy ? 'Enabling authentication…' : 'Enable authentication'}
					</Button>
				</div>
			</form>
		{/if}
	</section>

	<section aria-labelledby="security-https-heading" class={cardClass}>
		<div class="flex flex-wrap items-start justify-between gap-3">
			<div class="flex flex-col gap-1">
				<div class="flex items-center gap-2">
					<Lock class="size-4 text-fg-muted" aria-hidden="true" />
					<svelte:element
						this={headingTag}
						id="security-https-heading"
						class="text-base/7 font-semibold text-fg"
					>
						HTTPS
					</svelte:element>
				</div>
				<p class="text-sm/6 text-fg-muted">
					{#if isHttps}
						HTTPS is active on this HQ. Replace the TLS certificate below to rotate it live.
					{:else}
						Switch this HQ from plain HTTP to HTTPS using a generated self-signed certificate or
						your own PEM certificate and private key.
					{/if}
				</p>
			</div>
			<StatusPill
				label={listenSchemeLabel(listenScheme)}
				class={isHttps ? tone.ok : tone.neutral}
			/>
		</div>

		{#if tlsBlockedByAuth}
			<div
				role="status"
				data-testid="https-auth-guard"
				class="rounded-md bg-warning/10 p-3 text-sm/6 text-warning-text-strong ring-1 ring-warning/30"
			>
				Enable authentication above before turning on HTTPS.
			</div>
		{:else if !isAdmin && session}
			<p class="text-sm/6 text-fg-muted">Only the instance administrator can configure HTTPS.</p>
		{/if}

		{#if tlsError}
			<div
				role="alert"
				data-testid="https-error"
				class="border-l-4 border-danger bg-danger-emphasis/10 p-3 text-sm/6 text-danger-text-strong"
			>
				<p>{tlsError}</p>
			</div>
		{/if}

		{#if tlsResult}
			<div
				role="status"
				data-testid="https-result"
				class="flex flex-col gap-3 rounded-md bg-success-emphasis/10 p-4 text-sm/6 text-fg ring-1 ring-success-emphasis/20"
			>
				<p class="font-semibold text-success-text">
					{tlsResult.restart_required
						? 'HTTPS configured. Restart HQ to switch the browser listener.'
						: 'TLS certificate updated on the active HTTPS listener.'}
				</p>
				<dl class="flex flex-col gap-1.5 text-sm/6">
					<div class="flex flex-wrap gap-x-2">
						<dt class="text-fg-muted">Origin:</dt>
						<dd class="font-mono text-fg" data-testid="https-origin">{tlsResult.origin}</dd>
					</div>
					<div class="flex flex-wrap gap-x-2">
						<dt class="text-fg-muted">SHA-256 fingerprint:</dt>
						<dd class="font-mono break-all text-fg" data-testid="https-fingerprint">
							{tlsResult.fingerprint}
						</dd>
					</div>
					<div class="flex flex-wrap gap-x-2">
						<dt class="text-fg-muted">Source:</dt>
						<dd class="text-fg">
							{tlsResult.source === 'made' ? 'Self-signed certificate' : 'Supplied PEM certificate'}
						</dd>
					</div>
				</dl>
				{#if tlsResult.restart_required}
					<p class="text-sm/6 text-fg-secondary" data-testid="https-restart-note">
						Restart the HQ service on the HQ machine, then open
						<code class="font-mono text-fg">{tlsResult.origin}</code> in your browser.
					</p>
				{/if}
			</div>
		{/if}

		<form class="flex max-w-xl flex-col gap-4" novalidate onsubmit={submitEnableTls}>
			<fieldset class="flex flex-col gap-2" disabled={tlsDisabled}>
				<legend class="text-sm/6 font-medium text-fg">Certificate source</legend>
				<label class="flex items-center gap-2 text-sm/6 text-fg">
					<input
						type="radio"
						name="tls-mode"
						value="self_signed"
						bind:group={tlsMode}
						class="size-4 accent-primary"
					/>
					<span>Self-signed certificate (generated on the HQ machine)</span>
				</label>
				<label class="flex items-center gap-2 text-sm/6 text-fg">
					<input
						type="radio"
						name="tls-mode"
						value="supplied"
						bind:group={tlsMode}
						class="size-4 accent-primary"
					/>
					<span>Supplied PEM certificate and private key</span>
				</label>
			</fieldset>

			{#if tlsMode === 'supplied'}
				<div class="flex flex-col gap-1.5">
					<Label for="tls-cert">Certificate PEM</Label>
					<textarea
						id="tls-cert"
						rows={5}
						bind:value={tlsCert}
						disabled={tlsDisabled}
						placeholder="-----BEGIN CERTIFICATE-----"
						aria-invalid={tlsFieldErrors.cert ? 'true' : undefined}
						aria-describedby={tlsFieldErrors.cert ? 'tls-cert-error' : undefined}
						class="block w-full rounded-md bg-surface-sunken px-3 py-2 font-mono text-xs/5 text-fg outline-1 -outline-offset-1 outline-line focus:outline-2 focus:-outline-offset-2 focus:outline-ring disabled:opacity-60"
					></textarea>
					{#if tlsFieldErrors.cert}
						<p id="tls-cert-error" class="text-sm/6 text-danger" role="alert">
							{tlsFieldErrors.cert}
						</p>
					{/if}
				</div>

				<div class="flex flex-col gap-1.5">
					<Label for="tls-key">Private key PEM</Label>
					<textarea
						id="tls-key"
						rows={5}
						bind:value={tlsKey}
						disabled={tlsDisabled}
						placeholder="-----BEGIN PRIVATE KEY-----"
						aria-invalid={tlsFieldErrors.key ? 'true' : undefined}
						aria-describedby={tlsFieldErrors.key ? 'tls-key-error' : undefined}
						class="block w-full rounded-md bg-surface-sunken px-3 py-2 font-mono text-xs/5 text-fg outline-1 -outline-offset-1 outline-line focus:outline-2 focus:-outline-offset-2 focus:outline-ring disabled:opacity-60"
					></textarea>
					{#if tlsFieldErrors.key}
						<p id="tls-key-error" class="text-sm/6 text-danger" role="alert">
							{tlsFieldErrors.key}
						</p>
					{/if}
				</div>
			{/if}

			<div class="flex max-w-sm flex-col gap-1.5">
				<Label for="tls-hsts">Strict-Transport-Security (HSTS)</Label>
				<select
					id="tls-hsts"
					bind:value={tlsHsts}
					disabled={tlsDisabled}
					class="block w-full rounded-md bg-surface-sunken px-3 py-2 text-sm/6 text-fg outline-1 -outline-offset-1 outline-line focus:outline-2 focus:-outline-offset-2 focus:outline-ring disabled:opacity-60"
				>
					{#each ['off', 'on', 'clear'] as const as option (option)}
						<option value={option}>{hstsLabel(option)}</option>
					{/each}
				</select>
			</div>

			<div>
				<Button type="submit" disabled={tlsDisabled}>
					{#if tlsBusy}
						{isHttps ? 'Updating certificate…' : 'Enabling HTTPS…'}
					{:else}
						{isHttps ? 'Replace TLS certificate' : 'Enable HTTPS'}
					{/if}
				</Button>
			</div>
		</form>
	</section>
</div>
