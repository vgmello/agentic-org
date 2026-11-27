<!--
  @component
  Agent Profile › Overview BYOA external sign-in section (`TASK-249`, `doc-102` §4).
  Shows pending sign-in approvals with familiarity badges, the active `External` lease with
  `< 24h` expiring-soon warning, 1-click Renew (+7d), Revoke confirmation, and session history.
  Zero `$effect`: uses `{@attach}` for target sync + 30s countdown ticker and `onDestroy` teardown.
-->
<script lang="ts">
	import { onDestroy, untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import Lock from '@lucide/svelte/icons/lock';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import RotateCw from '@lucide/svelte/icons/rotate-cw';
	import ShieldOff from '@lucide/svelte/icons/shield-off';
	import Callout from '$lib/components/Callout.svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import ErrorState from '$lib/components/ErrorState.svelte';
	import Section from '$lib/components/Section.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { gutters, mobileButton } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { Button } from '$lib/components/ui/button';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import { ExternalAuthController } from '../../external-auth-controller.svelte';
	import {
		RENEW_7D_TTL_SECONDS,
		formatExternalAuthErrorMessage,
		formatHandle,
		formatTimeRemaining,
		isExpired,
		isExpiringSoon
	} from '../../external-auth-present';
	import type { ExternalSessionView, PendingExternalAuthView } from '../../external-auth-types';
	import { whenText } from '../../present';
	import type { MemberProfile } from '../../types';
	import ExternalAuthHistoryList from './ExternalAuthHistoryList.svelte';
	import ExternalAuthPendingList from './ExternalAuthPendingList.svelte';

	type ApproveDialogComponent = typeof import('./ExternalAuthApproveDialog.svelte').default;

	let {
		organizationId,
		profile,
		changes = 0,
		onChanged
	}: {
		organizationId: string;
		profile: MemberProfile;
		changes?: number;
		onChanged?: () => Promise<void>;
	} = $props();

	const controller = new ExternalAuthController();
	let sectionEl = $state<HTMLElement | null>(null);
	let nowMs = $state(Date.now());

	let ApproveDialog = $state.raw<ApproveDialogComponent | undefined>(undefined);
	let loadingApproveDialog = $state(false);
	let approveOpen = $state(false);
	let selectedPending = $state<PendingExternalAuthView | undefined>(undefined);
	let approveOpener = $state<HTMLElement | null>(null);

	let revokeOpen = $state(false);
	let revokeOpener = $state<HTMLElement | null>(null);
	let currentTargetKey = '';
	let lastLoadKey = '';
	let destroyed = false;

	const activeLease = $derived(
		controller.data?.active?.active ? controller.data.active : undefined
	);
	const pendingList = $derived(controller.data?.pending ?? []);
	const historyList = $derived(controller.data?.history ?? []);
	const activeExpiringSoon = $derived(
		activeLease ? isExpiringSoon(activeLease.expires_at, nowMs) : false
	);
	const activeExpired = $derived(activeLease ? isExpired(activeLease.expires_at, nowMs) : false);
	const handleLabel = $derived(formatHandle(profile.handle));
	const isEmpty = $derived(
		controller.status === 'ready' &&
			!activeLease &&
			pendingList.length === 0 &&
			historyList.length === 0
	);
	const revokeDescription = $derived(
		activeLease
			? `Revokes the active external lease for ${activeLease.workspace_label} and closes open office socket connections immediately. The workspace must run agent-os login again to reconnect.`
			: `Revokes ${profile.name}'s active external lease immediately.`
	);

	onDestroy(() => {
		destroyed = true;
		controller.dispose();
	});

	function attachLoad() {
		const sessionKind = appSession.state.kind;
		if (sessionKind === 'loading') return;
		const orgId = organizationId;
		const memId = profile.id;
		const rev = changes;
		const targetKey = `${orgId}:${memId}`;
		const loadKey = `${sessionKind}:${targetKey}:${rev}`;
		if (loadKey === lastLoadKey) return;
		const targetChanged = targetKey !== currentTargetKey;
		currentTargetKey = targetKey;
		lastLoadKey = loadKey;
		untrack(() => {
			if (targetChanged) {
				approveOpen = false;
				revokeOpen = false;
				selectedPending = undefined;
			}
			void controller.load(orgId, memId, { silent: !targetChanged });
		});
	}

	function attachTicker(node: HTMLElement) {
		sectionEl = node;
		nowMs = Date.now();
		const timer = setInterval(() => {
			nowMs = Date.now();
		}, 30_000);
		return () => {
			clearInterval(timer);
			if (sectionEl === node) sectionEl = null;
		};
	}

	async function openApproveFor(
		request: PendingExternalAuthView | undefined,
		from: HTMLElement | null
	) {
		const startKey = currentTargetKey;
		selectedPending = request;
		approveOpener = from;
		if (!ApproveDialog) {
			if (loadingApproveDialog) return;
			loadingApproveDialog = true;
			try {
				const mod = await import('./ExternalAuthApproveDialog.svelte');
				if (destroyed || currentTargetKey !== startKey) return;
				ApproveDialog = mod.default;
			} catch {
				toast.error('Could not open the approval dialog. Refresh and try again.');
				return;
			} finally {
				loadingApproveDialog = false;
			}
		}
		if (!destroyed && currentTargetKey === startKey) approveOpen = true;
	}

	async function refreshAfterMutation() {
		if (!onChanged) {
			await controller.reload();
			return;
		}
		try {
			await onChanged();
		} catch {
			await controller.reload();
		}
	}

	async function handleApproved(session: ExternalSessionView) {
		nowMs = Date.now();
		toast.success(`Approved external sign-in for ${profile.name}.`, {
			description: `${session.workspace_label} · expires ${whenText(session.expires_at)}`
		});
		await refreshAfterMutation();
	}

	async function handleRenew() {
		if (!activeLease || controller.busyAction) return;
		try {
			const renewed = await controller.renew(organizationId, profile.id, {
				session_id: activeLease.session_id,
				ttl_seconds: RENEW_7D_TTL_SECONDS
			});
			nowMs = Date.now();
			toast.success(`Renewed external lease for ${profile.name} (+7d).`, {
				description: `Expires ${whenText(renewed.expires_at)}`
			});
		} catch (err) {
			toast.error(`Couldn't renew external lease for ${profile.name}.`, {
				description: formatExternalAuthErrorMessage(
					controller.actionError ?? {
						source: 'api',
						code: 'unavailable',
						message: err instanceof Error ? err.message : 'External lease renewal failed.'
					}
				)
			});
		}
	}

	async function confirmRevoke() {
		const res = await controller.revoke(
			organizationId,
			profile.id,
			{ ...(activeLease ? { session_id: activeLease.session_id } : {}) },
			{ reload: !onChanged, recordActionError: false }
		);
		nowMs = Date.now();
		toast.success(
			res.revoked
				? `Revoked external lease for ${profile.name}.`
				: `No active external lease remained for ${profile.name}.`
		);
		if (onChanged) await refreshAfterMutation();
	}
</script>

<div {@attach attachLoad} {@attach attachTicker} data-testid="external-auth-section">
	<Section
		level={3}
		title="External sign-in (BYOA)"
		description="Authorize external workspaces via agent-os login, monitor active external leases, renew for +7d, or revoke access immediately."
	>
		{#snippet actions()}
			{#if controller.status !== 'unauthorized'}
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={controller.reloading || Boolean(controller.busyAction)}
					onclick={() => {
						nowMs = Date.now();
						void controller.reload();
					}}
				>
					<RefreshCw
						class={['size-4', controller.reloading && 'animate-spin motion-reduce:animate-none']}
						aria-hidden="true"
					/>
					<span>{controller.reloading ? 'Refreshing…' : 'Refresh'}</span>
				</Button>
				{#if controller.status === 'ready'}
					<Button
						type="button"
						variant="outline"
						class={mobileButton}
						disabled={Boolean(controller.busyAction) || loadingApproveDialog}
						onclick={(e) => void openApproveFor(pendingList[0], e.currentTarget)}
					>
						<KeyRound class="size-4" aria-hidden="true" />
						<span>Enter code…</span>
					</Button>
				{/if}
			{/if}
		{/snippet}

		{#if controller.status === 'idle' || controller.status === 'loading'}
			<SkeletonRows label="external sign-in sessions" rows={2} lines={2} />
		{:else if controller.status === 'unauthorized'}
			<div class={gutters}>
				<Callout role="status" data-testid="external-auth-unauthorized">
					{#snippet icon()}<Lock class="mt-0.5 size-4 text-fg-muted" />{/snippet}
					<p class="text-sm/6 text-fg-secondary">
						{controller.error
							? formatExternalAuthErrorMessage(controller.error)
							: 'Only authorized human operators can view or approve external sign-ins for this agent.'}
					</p>
				</Callout>
			</div>
		{:else if controller.status === 'error'}
			<ErrorState
				level={4}
				title="Couldn't load external sign-in sessions"
				message={controller.error ? formatExternalAuthErrorMessage(controller.error) : undefined}
				onretry={() => {
					nowMs = Date.now();
					void controller.reload();
				}}
				retrying={controller.reloading}
			/>
		{:else}
			<div class="{gutters} space-y-6">
				{#if controller.actionError}
					<Callout role="alert" class="border-danger/30 bg-danger-emphasis/10 text-danger-text">
						{#snippet icon()}<AlertTriangle class="mt-0.5 size-4 text-danger" />{/snippet}
						<div class="flex flex-wrap items-center justify-between gap-2">
							<p class="text-sm/6">{formatExternalAuthErrorMessage(controller.actionError)}</p>
							<Button
								type="button"
								variant="ghost"
								size="sm"
								onclick={() => controller.clearActionError()}>Dismiss</Button
							>
						</div>
					</Callout>
				{/if}

				{#if isEmpty}
					<div
						class="rounded-md border border-line bg-tint/[0.03] p-4 text-sm/6 text-fg-muted"
						data-testid="external-auth-empty"
					>
						<p class="font-medium text-fg-secondary">
							No external sign-in requests or active external lease.
						</p>
						<p class="mt-1 [overflow-wrap:anywhere]">
							Run <code class="font-mono text-fg-secondary"
								>agent-os login --as {handleLabel ?? profile.id}</code
							> in an external workspace to request approval.
						</p>
					</div>
				{:else}
					{#if pendingList.length > 0}
						<div class="space-y-2.5">
							<h4 class="text-sm/6 font-semibold text-fg">
								Pending approval requests ({pendingList.length})
							</h4>
							<ExternalAuthPendingList
								pending={pendingList}
								{nowMs}
								busy={Boolean(controller.busyAction) || loadingApproveDialog}
								onApprove={(req, opener) => void openApproveFor(req, opener)}
							/>
						</div>
					{/if}

					<div class="space-y-2.5" data-testid="external-auth-active-block">
						<h4 class="text-sm/6 font-semibold text-fg">Active external lease</h4>
						{#if activeLease}
							<div
								class="rounded-md border border-line bg-tint/[0.03] p-4"
								data-testid="external-auth-active-lease"
							>
								<div class="flex flex-wrap items-start justify-between gap-3">
									<div class="min-w-0 flex-1 basis-52 space-y-2">
										<div class="flex flex-wrap items-center gap-2">
											<StatusPill label="External" class={tone.info} />
											{#if activeExpiringSoon}
												<StatusPill label="Expiring soon (< 24h)" class={tone.owner}>
													{#snippet icon()}<AlertTriangle class="size-3.5" />{/snippet}
												</StatusPill>
											{:else if activeExpired}
												<StatusPill label="Expired" class={tone.bad} />
											{/if}
											{#if activeLease.took_over_managed}
												<StatusPill label="Took over managed turn" class={tone.neutral} />
											{/if}
										</div>
										<p class="text-sm/6 font-medium text-fg [overflow-wrap:anywhere]">
											{activeLease.workspace_label}
										</p>
									</div>

									<div class="flex flex-wrap items-center gap-2">
										<Button
											type="button"
											variant="outline"
											class={mobileButton}
											disabled={Boolean(controller.busyAction)}
											aria-busy={controller.busyAction === 'renew'}
											onclick={() => void handleRenew()}
										>
											<RotateCw class="size-4" aria-hidden="true" />
											<span>
												{controller.busyAction === 'renew' ? 'Renewing…' : 'Renew (+7d)'}
											</span>
										</Button>
										<Button
											type="button"
											variant="outline"
											class="{mobileButton} border-danger/40 text-danger-text hover:bg-danger-emphasis/10"
											disabled={Boolean(controller.busyAction)}
											onclick={(e) => {
												revokeOpener = e.currentTarget;
												revokeOpen = true;
											}}
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
											{whenText(activeLease.expires_at)} · {formatTimeRemaining(
												activeLease.expires_at,
												nowMs
											)}
										</dd>
									</div>
									<div class="min-w-0">
										<dt class="font-medium text-fg-secondary">Approved</dt>
										<dd class="text-fg-muted [overflow-wrap:anywhere]">
											{activeLease.approved_by} · {whenText(activeLease.approved_at)}
										</dd>
									</div>
									<div class="min-w-0">
										<dt class="font-medium text-fg-secondary">Public key fingerprint</dt>
										<dd class="font-mono text-fg-muted [overflow-wrap:anywhere]">
											{activeLease.pubkey_fingerprint}
										</dd>
									</div>
									<div class="min-w-0 sm:col-span-2 lg:col-span-3">
										<dt class="font-medium text-fg-secondary">Session ID</dt>
										<dd class="font-mono text-fg-muted [overflow-wrap:anywhere]">
											{activeLease.session_id}
										</dd>
									</div>
								</dl>
							</div>
						{:else}
							<p class="text-sm/6 text-fg-muted">No active external lease.</p>
						{/if}
					</div>

					<ExternalAuthHistoryList history={historyList} />
				{/if}
			</div>
		{/if}
	</Section>
</div>

{#if ApproveDialog}
	<ApproveDialog
		bind:open={approveOpen}
		{organizationId}
		agentName={profile.name}
		agentHandle={profile.handle}
		pending={selectedPending}
		opener={approveOpener}
		fallback={() => sectionEl?.querySelector<HTMLElement>('button') ?? null}
		onApproved={handleApproved}
	/>
{/if}

<ConfirmDialog
	bind:open={revokeOpen}
	title="Revoke external lease for {profile.name}?"
	description={revokeDescription}
	confirmLabel="Revoke lease"
	busyLabel="Revoking…"
	destructive
	opener={revokeOpener}
	fallback={() => sectionEl?.querySelector<HTMLElement>('button') ?? null}
	onconfirm={confirmRevoke}
/>
