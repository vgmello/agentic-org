<!--
  @component
  Pending BYOA external-agent sign-in requests list (`TASK-249`, `doc-102` §4).
  Shows workspace familiarity badges, last-approved context, masked code prefix (`XXXX-••••`),
  office/host/workspace/fingerprint metadata, expiry countdown, and Approve action.
-->
<script lang="ts">
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Clock from '@lucide/svelte/icons/clock';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { Button } from '$lib/components/ui/button';
	import {
		familiarityMeta,
		formatHandle,
		formatLastApproved,
		formatTimeRemaining,
		isExpired
	} from '../../external-auth-present';
	import type { PendingExternalAuthView } from '../../external-auth-types';

	let {
		pending,
		nowMs,
		busy = false,
		membersById,
		onApprove
	}: {
		pending: readonly PendingExternalAuthView[];
		nowMs: number;
		busy?: boolean;
		membersById?: ReadonlyMap<string, string>;
		onApprove: (request: PendingExternalAuthView, opener: HTMLElement | null) => void;
	} = $props();
</script>

<ul role="list" class="space-y-3" data-testid="external-auth-pending-list">
	{#each pending as req (req.request_id)}
		{@const fam = familiarityMeta(req.workspace_familiarity)}
		{@const lastApproved = formatLastApproved(
			req.last_approved_by,
			req.last_approved_at,
			membersById
		)}
		{@const handleLabel = formatHandle(req.handle)}
		{@const expired = isExpired(req.expires_at, nowMs)}
		{@const remaining = formatTimeRemaining(req.expires_at, nowMs)}
		<li
			class="rounded-md border border-line bg-tint/[0.03] p-4"
			data-testid="external-auth-pending-card"
			data-request-id={req.request_id}
			data-familiarity={req.workspace_familiarity}
		>
			<div class="flex flex-wrap items-start justify-between gap-3">
				<div class="min-w-0 flex-1 basis-52 space-y-2">
					<div class="flex flex-wrap items-center gap-2">
						<StatusPill label={fam.label} class={fam.toneClass}>
							{#snippet icon()}
								{#if fam.severity === 'familiar'}
									<ShieldCheck class="size-3.5" />
								{:else if fam.severity === 'caution'}
									<ShieldAlert class="size-3.5" />
								{:else}
									<AlertTriangle class="size-3.5" />
								{/if}
							{/snippet}
						</StatusPill>
						{#if req.managed_turn_active}
							<StatusPill label="Managed turn active" class={tone.owner} />
						{/if}
						<StatusPill label={remaining} class={expired ? tone.bad : tone.neutral}>
							{#snippet icon()}<Clock class="size-3.5" />{/snippet}
						</StatusPill>
					</div>

					<p class="text-sm/6 font-medium text-fg [overflow-wrap:anywhere]">
						{req.workspace_label}
						{#if handleLabel}
							<span class="font-normal text-fg-muted"> · {handleLabel}</span>
						{/if}
					</p>

					{#if lastApproved}
						<p class="text-xs/5 text-fg-secondary [overflow-wrap:anywhere]">{lastApproved}</p>
					{/if}
				</div>

				<div class="flex shrink-0 flex-wrap items-center gap-2">
					<Button
						type="button"
						class={mobileButton}
						disabled={busy || expired}
						aria-label="Approve external sign-in for {req.workspace_label}"
						onclick={(e) => onApprove(req, e.currentTarget)}
					>
						<KeyRound class="size-4" aria-hidden="true" />
						<span>{expired ? 'Expired' : 'Approve…'}</span>
					</Button>
				</div>
			</div>

			<dl
				class="mt-3 grid grid-cols-1 gap-x-6 gap-y-2 border-t border-line-subtle pt-3 text-xs/5 sm:grid-cols-2 lg:grid-cols-3"
			>
				<div class="min-w-0">
					<dt class="font-medium text-fg-secondary">Lookup code</dt>
					<dd class="font-mono text-fg [overflow-wrap:anywhere]">{req.code_prefix}</dd>
				</div>
				<div class="min-w-0">
					<dt class="font-medium text-fg-secondary">Office</dt>
					<dd class="text-fg-muted [overflow-wrap:anywhere]">{req.office_name}</dd>
				</div>
				{#if req.hostname}
					<div class="min-w-0">
						<dt class="font-medium text-fg-secondary">Host</dt>
						<dd class="font-mono text-fg-muted [overflow-wrap:anywhere]">{req.hostname}</dd>
					</div>
				{/if}
				{#if req.workspace_path}
					<div class="min-w-0 sm:col-span-2">
						<dt class="font-medium text-fg-secondary">Workspace path</dt>
						<dd class="font-mono text-fg-muted [overflow-wrap:anywhere]">{req.workspace_path}</dd>
					</div>
				{/if}
				<div class="min-w-0">
					<dt class="font-medium text-fg-secondary">Public key fingerprint</dt>
					<dd class="font-mono text-fg-muted [overflow-wrap:anywhere]">{req.pubkey_fingerprint}</dd>
				</div>
			</dl>
		</li>
	{/each}
</ul>
