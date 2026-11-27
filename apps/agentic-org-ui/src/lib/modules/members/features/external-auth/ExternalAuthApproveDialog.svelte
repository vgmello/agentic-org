<!--
  @component
  Human-only approval dialog for BYOA external-agent sign-in (`TASK-249`, `doc-102` §4).
  Accepts the 8-character Crockford approval code (`XXXX-YYYY`), lease TTL (`8h`/`24h`/`7d`), and
  explicit takeover confirmation when a live managed turn is running (or on `409 conflict`).
  Never logs or retains the approval code after submission, close, or unmount.
-->
<script lang="ts">
	import { onDestroy } from 'svelte';
	import { beforeNavigate } from '$app/navigation';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import Callout from '$lib/components/Callout.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { errorClass, helpClass, inputClass, mobileButton } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
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
	import { registerModalBack } from '$lib/platform/workspace/modal-back';
	import { asExternalAuthError, externalAuthApi } from '../../external-auth-http';
	import {
		DEFAULT_LEASE_TTL_SECONDS,
		EXTERNAL_AUTH_TTL_PRESETS,
		codePrefixMismatch,
		familiarityMeta,
		formatApprovalCodeAssist,
		formatExternalAuthErrorMessage,
		formatHandle,
		formatLastApproved,
		validateApprovalCode
	} from '../../external-auth-present';
	import type {
		ExternalAuthApi,
		ExternalSessionView,
		PendingExternalAuthView
	} from '../../external-auth-types';

	let {
		open = $bindable(false),
		organizationId,
		agentName,
		agentHandle,
		pending,
		opener,
		fallback,
		onCloseAutoFocus,
		api = externalAuthApi,
		onApproved
	}: {
		open: boolean;
		organizationId: string;
		agentName: string;
		agentHandle?: string;
		pending?: PendingExternalAuthView;
		opener?: HTMLElement | null;
		fallback?: () => HTMLElement | null;
		onCloseAutoFocus?: (event: Event) => void;
		api?: Pick<ExternalAuthApi, 'approve'>;
		onApproved?: (session: ExternalSessionView) => void | Promise<void>;
	} = $props();

	const uid = $props.id();
	const codeId = `${uid}-code`;
	const takeoverId = `${uid}-takeover`;

	let code = $state('');
	let ttlSeconds = $state<number>(DEFAULT_LEASE_TTL_SECONDS);
	let takeoverConfirmed = $state(false);
	let conflictRequiresTakeover = $state(false);
	let busy = $state(false);
	let errorMessage = $state<string | undefined>(undefined);
	let codeInputEl = $state<HTMLInputElement | null>(null);

	let submitAbort: AbortController | undefined;
	let destroyed = false;

	const handleLabel = $derived(formatHandle(pending?.handle ?? agentHandle));
	const familiarity = $derived(
		pending ? familiarityMeta(pending.workspace_familiarity) : undefined
	);
	const lastApproved = $derived(
		pending ? formatLastApproved(pending.last_approved_by, pending.last_approved_at) : undefined
	);
	const needsTakeover = $derived(Boolean(pending?.managed_turn_active || conflictRequiresTakeover));
	const prefixMismatch = $derived(codePrefixMismatch(code, pending?.code_prefix));
	const canSubmit = $derived(
		!busy && Boolean(code.trim()) && (!needsTakeover || takeoverConfirmed)
	);

	function resetDialog() {
		code = '';
		ttlSeconds = DEFAULT_LEASE_TTL_SECONDS;
		takeoverConfirmed = false;
		conflictRequiresTakeover = false;
		busy = false;
		errorMessage = undefined;
	}

	function dismissIfIdle() {
		if (busy) return;
		code = '';
		open = false;
	}

	function attachModalBack() {
		return registerModalBack(dismissIfIdle, { locked: () => busy });
	}

	beforeNavigate((navigation) => {
		if (
			window.navigation ||
			!open ||
			navigation.type !== 'popstate' ||
			(navigation.delta ?? 0) >= 0
		) {
			return;
		}
		navigation.cancel();
		dismissIfIdle();
	});

	onDestroy(() => {
		destroyed = true;
		submitAbort?.abort();
		submitAbort = undefined;
		code = '';
	});

	function handleCodeInput(event: Event) {
		const target = event.currentTarget as HTMLInputElement;
		const formatted = formatApprovalCodeAssist(target.value);
		code = formatted;
		if (target.value !== formatted) target.value = formatted;
		if (errorMessage) errorMessage = undefined;
	}

	function prepareSubmitCode(): string | undefined {
		const validated = validateApprovalCode(code);
		if (!validated.ok) {
			errorMessage = validated.message;
			codeInputEl?.focus();
			return undefined;
		}
		if (needsTakeover && !takeoverConfirmed) {
			errorMessage = 'Confirm takeover of the active managed turn before approving.';
			return undefined;
		}
		return validated.code;
	}

	function applySubmitError(err: unknown) {
		const parsed = asExternalAuthError(err, 'External sign-in approval was refused.');
		if (parsed.code === 'conflict') {
			conflictRequiresTakeover = true;
			errorMessage =
				'A managed turn is currently running for this agent. Confirm takeover below and approve again to park it.';
			return;
		}
		if (parsed.code === 'expired') code = '';
		errorMessage = formatExternalAuthErrorMessage(parsed);
	}

	async function notifyApproved(session: ExternalSessionView) {
		try {
			await onApproved?.(session);
		} catch {
			// Post-approval refresh failures never revert or misattribute an already-consumed code.
		}
	}

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		const normalizedCode = prepareSubmitCode();
		if (!normalizedCode) return;

		submitAbort?.abort();
		const abort = new AbortController();
		submitAbort = abort;
		busy = true;
		errorMessage = undefined;
		const requestTakeover = needsTakeover && takeoverConfirmed;
		const isAlive = () => !destroyed && !abort.signal.aborted;

		let session: ExternalSessionView;
		try {
			session = await api.approve(
				organizationId,
				{ code: normalizedCode, ttl_seconds: ttlSeconds, takeover: requestTakeover },
				abort.signal
			);
		} catch (err) {
			if (submitAbort === abort) submitAbort = undefined;
			busy = false;
			if (isAlive()) applySubmitError(err);
			return;
		}

		// Immediately wipe the one-time approval code and close before running post-approval refresh.
		code = '';
		if (submitAbort === abort) submitAbort = undefined;
		busy = false;
		if (!isAlive()) return;
		open = false;
		await notifyApproved(session);
	}
</script>

<Dialog
	bind:open
	onOpenChange={(next) => {
		if (!next) {
			if (busy) {
				open = true;
				return;
			}
			code = '';
		}
	}}
	onOpenChangeComplete={(next) => {
		if (!next) resetDialog();
	}}
>
	<DialogContent
		onOpenAutoFocus={(event) => {
			resetDialog();
			event.preventDefault();
			codeInputEl?.focus();
		}}
		{onCloseAutoFocus}
		{opener}
		{fallback}
		class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-lg"
	>
		<div {@attach attachModalBack} class="contents">
			<DialogHeader>
				<DialogTitle text="Approve external sign-in" />
				<DialogDescription class="[overflow-wrap:anywhere]">
					Enter the 8-character approval code printed by <code class="font-mono"
						>agent-os login</code
					>
					for {handleLabel ? `${agentName} (${handleLabel})` : agentName}. Approving supersedes any
					existing external lease for this agent.
				</DialogDescription>
			</DialogHeader>

			{#if pending}
				<div class="space-y-2.5 rounded-md border border-line bg-tint/[0.03] p-3 text-xs/5">
					<div class="flex flex-wrap items-center gap-2">
						{#if familiarity}
							<StatusPill label={familiarity.label} class={familiarity.toneClass} />
						{/if}
						{#if pending.managed_turn_active}
							<StatusPill label="Managed turn active" class={tone.owner} />
						{/if}
						<span class="font-mono text-fg-secondary">Lookup {pending.code_prefix}</span>
					</div>
					{#if familiarity}
						<p class="text-fg-muted [overflow-wrap:anywhere]">{familiarity.description}</p>
					{/if}
					{#if lastApproved}
						<p class="text-fg-secondary [overflow-wrap:anywhere]">{lastApproved}</p>
					{/if}
					<dl class="grid grid-cols-1 gap-1.5 pt-1 text-fg-muted sm:grid-cols-2">
						<div class="min-w-0">
							<dt class="font-medium text-fg-secondary">Office</dt>
							<dd class="[overflow-wrap:anywhere]">{pending.office_name}</dd>
						</div>
						{#if pending.hostname}
							<div class="min-w-0">
								<dt class="font-medium text-fg-secondary">Host</dt>
								<dd class="font-mono [overflow-wrap:anywhere]">{pending.hostname}</dd>
							</div>
						{/if}
						<div class="min-w-0 sm:col-span-2">
							<dt class="font-medium text-fg-secondary">Workspace</dt>
							<dd class="[overflow-wrap:anywhere]">
								<span class="font-medium text-fg-secondary">{pending.workspace_label}</span>
								{#if pending.workspace_path && pending.workspace_path !== pending.workspace_label}
									<span class="block font-mono text-fg-muted">{pending.workspace_path}</span>
								{/if}
							</dd>
						</div>
						<div class="min-w-0 sm:col-span-2">
							<dt class="font-medium text-fg-secondary">Public key fingerprint</dt>
							<dd class="font-mono [overflow-wrap:anywhere]">{pending.pubkey_fingerprint}</dd>
						</div>
					</dl>
				</div>
			{/if}

			<form class="space-y-4" onsubmit={handleSubmit} novalidate>
				<div>
					<Label for={codeId} class="text-sm/6 font-medium text-fg">Approval code</Label>
					<Input
						bind:ref={codeInputEl}
						id={codeId}
						name="approval_code"
						type="text"
						autocomplete="off"
						autocapitalize="characters"
						spellcheck={false}
						maxlength={32}
						placeholder={pending?.code_prefix ?? 'XXXX-YYYY'}
						value={code}
						oninput={handleCodeInput}
						disabled={busy}
						aria-invalid={errorMessage ? 'true' : undefined}
						class="{inputClass} mt-1.5 font-mono tracking-wider uppercase"
					/>
					<p class={helpClass}>
						Check the terminal output for the full 8-character code. The browser never receives the
						secret second half.
					</p>
					{#if prefixMismatch && pending}
						<p class="mt-2 text-xs/5 text-warning-text-strong" role="status">
							Prefix differs from selected request ({pending.code_prefix}). Double-check you are
							approving the intended terminal.
						</p>
					{/if}
				</div>

				<fieldset class="space-y-2" disabled={busy}>
					<legend class="text-sm/6 font-medium text-fg">Lease duration</legend>
					<div class="grid grid-cols-3 gap-2">
						{#each EXTERNAL_AUTH_TTL_PRESETS as preset (preset.id)}
							<label
								class={[
									'flex min-h-10 cursor-pointer items-center justify-center rounded-md border px-2 py-2 text-center text-sm font-medium transition-colors has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-offset-2 has-[:focus-visible]:outline-focus max-[639px]:min-h-11 motion-reduce:transition-none',
									ttlSeconds === preset.seconds
										? 'border-brand bg-primary/15 text-fg ring-1 ring-brand'
										: 'border-line bg-surface-raised text-fg-secondary hover:border-line-strong hover:text-fg'
								]}
							>
								<input
									type="radio"
									name="external_auth_ttl"
									value={preset.seconds}
									checked={ttlSeconds === preset.seconds}
									onchange={() => (ttlSeconds = preset.seconds)}
									class="sr-only"
								/>
								<span>{preset.shortLabel}</span>
							</label>
						{/each}
					</div>
					<p class={helpClass}>Active leases can be renewed for +7d or revoked at any time.</p>
				</fieldset>

				{#if needsTakeover}
					<Callout class="border-warning/40 bg-warning/10 text-warning-text-strong">
						{#snippet icon()}<AlertTriangle class="mt-0.5 size-4 text-warning" />{/snippet}
						<div class="space-y-2">
							<p class="text-xs/5 font-medium">
								A managed turn is currently running for {agentName}. Approving with takeover parks
								the managed turn and hands control to this external workspace.
							</p>
							<label
								for={takeoverId}
								class="flex cursor-pointer items-start gap-2.5 text-xs/5 text-fg"
							>
								<Checkbox
									id={takeoverId}
									bind:checked={takeoverConfirmed}
									disabled={busy}
									class="mt-0.5"
								/>
								<span class="[overflow-wrap:anywhere]">
									Park the active managed turn and hand off control to this external session
								</span>
							</label>
						</div>
					</Callout>
				{/if}

				{#if errorMessage}
					<p class="{errorClass} [overflow-wrap:anywhere]" role="alert">{errorMessage}</p>
				{/if}

				<DialogFooter class="gap-2 pt-2">
					<Button
						type="button"
						variant="outline"
						class={mobileButton}
						disabled={busy}
						onclick={() => (open = false)}>Cancel</Button
					>
					<Button type="submit" class={mobileButton} disabled={!canSubmit} aria-busy={busy}>
						<ShieldCheck class="size-4" aria-hidden="true" />
						<span>{busy ? 'Approving…' : needsTakeover ? 'Approve with takeover' : 'Approve'}</span>
					</Button>
				</DialogFooter>
			</form>
		</div>
	</DialogContent>
</Dialog>
