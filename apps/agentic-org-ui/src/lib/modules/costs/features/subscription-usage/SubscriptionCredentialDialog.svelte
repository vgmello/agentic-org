<!--
  @component
  Manage a private usage-API credential for one provider subscription connection (TASK-79).
  Secrets are sent directly to the office, stored outside organization Git, and never echoed back.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { errorClass, helpClass, inputClass, mobileButton } from '$lib/components/classes';
	import { toAppError } from '$lib/proto';
	import { costsApi } from '../../api';
	import type { SubscriptionUsageSnapshot } from '../../types';
	import { providerLabel } from './present';

	let {
		organizationId,
		onupdated,
		onCloseAutoFocus
	}: {
		organizationId: string;
		onupdated: (message: string) => void;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	const id = $props.id();
	let open = $state(false);
	let target = $state<SubscriptionUsageSnapshot | undefined>(undefined);
	let secret = $state('');
	let accountHint = $state('');
	let expiresDate = $state('');
	let expectedOrigin = $state('');
	let secretError = $state<string | undefined>(undefined);
	let originError = $state<string | undefined>(undefined);
	let formError = $state<string | undefined>(undefined);
	let busy = $state<'saving' | 'revoking' | undefined>(undefined);
	let secretInputEl = $state<HTMLInputElement | null>(null);
	let originInputEl = $state<HTMLInputElement | null>(null);

	export function start(snapshot: SubscriptionUsageSnapshot) {
		target = snapshot;
		secret = '';
		accountHint = snapshot.accountHint ?? '';
		expiresDate = '';
		expectedOrigin = '';
		secretError = undefined;
		originError = undefined;
		formError = undefined;
		busy = undefined;
		open = true;
	}

	function validateOrigin(raw: string): string | undefined {
		const trimmed = raw.trim();
		if (!trimmed) return undefined;
		try {
			const parsed = new URL(trimmed);
			if (parsed.protocol !== 'https:') return 'Expected origin must use https://.';
			if (parsed.pathname !== '/' || parsed.search || parsed.hash) {
				return 'Enter only the origin, such as https://api.anthropic.com.';
			}
			return undefined;
		} catch {
			return 'Enter a valid HTTPS origin, such as https://api.anthropic.com.';
		}
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!target || busy) return;
		const trimmedSecret = secret.trim();
		secretError = trimmedSecret ? undefined : 'Enter the private usage credential or token.';
		originError = validateOrigin(expectedOrigin);
		formError = undefined;
		if (secretError) {
			await tick();
			secretInputEl?.focus();
			return;
		}
		if (originError) {
			await tick();
			originInputEl?.focus();
			return;
		}
		busy = 'saving';
		try {
			const trimmedHint = accountHint.trim();
			const trimmedOrigin = expectedOrigin.trim();
			const expiresAt = expiresDate.trim() ? `${expiresDate.trim()}T23:59:59.000Z` : undefined;
			await costsApi.setSubscriptionUsageCredential(organizationId, {
				connectionId: target.connectionId,
				secret: trimmedSecret,
				...(trimmedHint ? { accountHint: trimmedHint } : {}),
				...(expiresAt ? { expiresAt } : {}),
				...(trimmedOrigin ? { expectedOrigin: trimmedOrigin } : {})
			});
			const name = target.connectionName;
			secret = '';
			busy = undefined;
			open = false;
			onupdated(`Usage credential saved for ${name}.`);
		} catch (e) {
			busy = undefined;
			formError = toAppError(e).message;
		}
	}

	async function revoke() {
		if (!target || busy) return;
		busy = 'revoking';
		formError = undefined;
		try {
			await costsApi.revokeSubscriptionUsageCredential(organizationId, target.connectionId);
			const name = target.connectionName;
			secret = '';
			busy = undefined;
			open = false;
			onupdated(`Usage credential revoked for ${name}.`);
		} catch (e) {
			busy = undefined;
			formError = toAppError(e).message;
		}
	}

	function openChanged(next: boolean) {
		if (!next) {
			if (busy) {
				open = true;
				return;
			}
			secret = '';
		}
	}
</script>

<Dialog bind:open onOpenChange={openChanged}>
	<DialogContent {onCloseAutoFocus} class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-lg">
		{#if target}
			<form class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-5" onsubmit={submit} novalidate>
				<DialogHeader>
					<DialogTitle text="Manage usage credential" />
					<DialogDescription class="[overflow-wrap:anywhere]">
						{target.connectionName} ({providerLabel(target.providerType)}) · Stored privately on
						this office outside organization Git. Secrets are never shown again after saving.
					</DialogDescription>
				</DialogHeader>

				<fieldset disabled={!!busy} class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-4">
					<div>
						<Label for="{id}-secret" class="block text-sm/6 font-medium text-fg">
							Usage API credential
						</Label>
						<Input
							bind:ref={secretInputEl}
							id="{id}-secret"
							type="password"
							autocomplete="off"
							bind:value={secret}
							aria-invalid={secretError ? 'true' : undefined}
							aria-describedby={secretError ? `${id}-secret-error` : `${id}-secret-help`}
							placeholder={target.hasUsageCredential
								? 'Enter a new credential to replace the saved one'
								: 'Paste your provider usage credential'}
							class="mt-1.5 {inputClass}"
						/>
						{#if secretError}
							<p id="{id}-secret-error" class={errorClass}>{secretError}</p>
						{:else}
							<p id="{id}-secret-help" class={helpClass}>
								Used only to collect provider-reported subscription usage counts.
							</p>
						{/if}
					</div>

					<div>
						<Label for="{id}-hint" class="block text-sm/6 font-medium text-fg">
							Account hint (optional)
						</Label>
						<Input
							id="{id}-hint"
							type="text"
							autocomplete="off"
							bind:value={accountHint}
							placeholder="e.g. ops@lumen.dev or Team Pro"
							class="mt-1.5 {inputClass}"
						/>
					</div>

					<div class="grid grid-cols-[minmax(0,1fr)] gap-4 min-[480px]:grid-cols-2">
						<div>
							<Label for="{id}-expires" class="block text-sm/6 font-medium text-fg">
								Expires on (optional)
							</Label>
							<Input
								id="{id}-expires"
								type="date"
								bind:value={expiresDate}
								class="mt-1.5 {inputClass}"
							/>
						</div>
						<div>
							<Label for="{id}-origin" class="block text-sm/6 font-medium text-fg">
								Expected origin (optional)
							</Label>
							<Input
								bind:ref={originInputEl}
								id="{id}-origin"
								type="url"
								autocomplete="off"
								bind:value={expectedOrigin}
								aria-invalid={originError ? 'true' : undefined}
								aria-describedby={originError ? `${id}-origin-error` : undefined}
								placeholder="https://api.anthropic.com"
								class="mt-1.5 {inputClass}"
							/>
							{#if originError}
								<p id="{id}-origin-error" class={errorClass}>{originError}</p>
							{/if}
						</div>
					</div>
				</fieldset>

				{#if formError}
					<p
						role="alert"
						class="rounded-md border border-line bg-surface-raised px-3 py-2 text-sm/6 text-fg"
					>
						{formError}
					</p>
				{/if}

				<DialogFooter class="flex-wrap gap-2 sm:justify-between">
					<div>
						{#if target.hasUsageCredential}
							<Button
								type="button"
								variant="outline"
								disabled={!!busy}
								onclick={() => void revoke()}
								class={mobileButton}
							>
								{busy === 'revoking' ? 'Revoking…' : 'Revoke credential'}
							</Button>
						{/if}
					</div>
					<div class="flex flex-wrap items-center gap-2">
						<Button
							type="button"
							variant="outline"
							disabled={!!busy}
							onclick={() => {
								open = false;
								openChanged(false);
							}}
							class={mobileButton}
						>
							Cancel
						</Button>
						<Button type="submit" disabled={!!busy} class={mobileButton}>
							{busy === 'saving' ? 'Saving…' : 'Save credential'}
						</Button>
					</div>
				</DialogFooter>
			</form>
		{/if}
	</DialogContent>
</Dialog>
