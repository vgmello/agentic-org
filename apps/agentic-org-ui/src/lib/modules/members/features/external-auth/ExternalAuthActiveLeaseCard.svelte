<!--
  @component
  Active BYOA external lease card (`TASK-249`, `doc-102` §4): displays the `External` badge,
  `< 24h` expiring-soon warning, Renew (+7d) and destructive Revoke… actions, and lease metadata.
-->
<script lang="ts">
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import RotateCw from '@lucide/svelte/icons/rotate-cw';
	import ShieldOff from '@lucide/svelte/icons/shield-off';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { Button } from '$lib/components/ui/button';
	import {
		formatExternalAuthTime,
		formatTimeRemaining,
		isExpired,
		isExpiringSoon,
		resolveExternalAuthActor
	} from '$lib/platform/workspace/external-auth';
	import type { ExternalAuthBusyAction } from '../../external-auth-controller.svelte';
	import type { ExternalSessionView } from '../../external-auth-types';

	let {
		lease,
		nowMs,
		busyAction,
		membersById,
		onRenew,
		onRevoke
	}: {
		lease?: ExternalSessionView;
		nowMs: number;
		busyAction?: ExternalAuthBusyAction;
		membersById?: ReadonlyMap<string, string>;
		onRenew: () => void;
		onRevoke: (opener: HTMLElement | null) => void;
	} = $props();

	const expiringSoon = $derived(lease ? isExpiringSoon(lease.expires_at, nowMs) : false);
	const expired = $derived(lease ? isExpired(lease.expires_at, nowMs) : false);
</script>

<div class="space-y-2.5" data-testid="external-auth-active-block">
	<h4 class="text-sm/6 font-semibold text-fg">Active external lease</h4>
	{#if lease}
		<div
			class="rounded-md border border-line bg-tint/[0.03] p-4"
			data-testid="external-auth-active-lease"
		>
			<div class="flex flex-wrap items-start justify-between gap-3">
				<div class="min-w-0 flex-1 basis-52 space-y-2">
					<div class="flex flex-wrap items-center gap-2">
						<StatusPill label="External" class={tone.info} />
						{#if expiringSoon}
							<StatusPill label="Expiring soon (< 24h)" class={tone.owner}>
								{#snippet icon()}<AlertTriangle class="size-3.5" />{/snippet}
							</StatusPill>
						{:else if expired}
							<StatusPill label="Expired" class={tone.bad} />
						{/if}
						{#if lease.took_over_managed}
							<StatusPill label="Took over managed turn" class={tone.neutral} />
						{/if}
					</div>
					<p class="text-sm/6 font-medium text-fg [overflow-wrap:anywhere]">
						{lease.workspace_label}
					</p>
				</div>

				<div class="flex flex-wrap items-center gap-2">
					<Button
						type="button"
						variant="outline"
						class={mobileButton}
						disabled={Boolean(busyAction)}
						aria-busy={busyAction === 'renew'}
						onclick={onRenew}
					>
						<RotateCw class="size-4" aria-hidden="true" />
						<span>{busyAction === 'renew' ? 'Renewing…' : 'Renew (+7d)'}</span>
					</Button>
					<Button
						type="button"
						variant="outline"
						class="{mobileButton} border-danger/40 text-danger-text hover:bg-danger-emphasis/10"
						disabled={Boolean(busyAction)}
						onclick={(e) => onRevoke(e.currentTarget)}
					>
						<ShieldOff class="size-4" aria-hidden="true" />
						<span>Revoke…</span>
					</Button>
				</div>
			</div>

			<dl
				class="mt-3 grid grid-cols-1 gap-x-6 gap-y-2 border-t border-line-subtle pt-3 text-xs/5 sm:grid-cols-2 lg:grid-cols-3"
			>
				<div class="min-w-0">
					<dt class="font-medium text-fg-secondary">Expires</dt>
					<dd class="text-fg-muted [overflow-wrap:anywhere]">
						{formatExternalAuthTime(lease.expires_at)} · {formatTimeRemaining(
							lease.expires_at,
							nowMs
						)}
					</dd>
				</div>
				<div class="min-w-0">
					<dt class="font-medium text-fg-secondary">Approved</dt>
					<dd class="text-fg-muted [overflow-wrap:anywhere]">
						{resolveExternalAuthActor(lease.approved_by, membersById) ?? 'Unknown member'} · {formatExternalAuthTime(
							lease.approved_at
						)}
					</dd>
				</div>
				<div class="min-w-0">
					<dt class="font-medium text-fg-secondary">Public key fingerprint</dt>
					<dd class="font-mono text-fg-muted [overflow-wrap:anywhere]">
						{lease.pubkey_fingerprint}
					</dd>
				</div>
				<div class="min-w-0 sm:col-span-2 lg:col-span-3">
					<dt class="font-medium text-fg-secondary">Session ID</dt>
					<dd class="font-mono text-fg-muted [overflow-wrap:anywhere]">{lease.session_id}</dd>
				</div>
			</dl>
		</div>
	{:else}
		<p class="text-sm/6 text-fg-muted">No active external lease.</p>
	{/if}
</div>
