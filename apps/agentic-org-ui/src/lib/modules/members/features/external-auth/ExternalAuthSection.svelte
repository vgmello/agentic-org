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
	import Callout from '$lib/components/Callout.svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import ErrorState from '$lib/components/ErrorState.svelte';
	import Section from '$lib/components/Section.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import { gutters, mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import {
		buildMemberNameMap,
		canUseExternalSignIn,
		formatApprovedSessionToast,
		formatExternalAuthTime
	} from '$lib/platform/workspace/external-auth';
	import type { Member } from '$lib/proto';
	import { ExternalAuthController } from '../../external-auth-controller.svelte';
	import { asExternalAuthError } from '../../external-auth-http';
	import {
		RENEW_7D_TTL_SECONDS,
		formatExternalAuthErrorMessage,
		formatHandle
	} from '../../external-auth-present';
	import type { ExternalSessionView, PendingExternalAuthView } from '../../external-auth-types';
	import type { MemberProfile } from '../../types';
	import ExternalAuthActiveLeaseCard from './ExternalAuthActiveLeaseCard.svelte';
	import ExternalAuthHistoryList from './ExternalAuthHistoryList.svelte';
	import ExternalAuthPendingList from './ExternalAuthPendingList.svelte';

	type ApproveDialogComponent = typeof import('./ExternalAuthApproveDialog.svelte').default;

	let {
		organizationId,
		profile,
		snapshotMembers = [],
		changes = 0,
		onChanged
	}: {
		organizationId: string;
		profile: MemberProfile;
		snapshotMembers?: readonly Member[];
		changes?: number;
		onChanged?: () => Promise<void>;
	} = $props();

	const controller = new ExternalAuthController();
	let sectionEl = $state<HTMLElement | null>(null);
	let nowMs = $state(Date.now());

	let ApproveDialog = $state.raw<ApproveDialogComponent | undefined>(undefined);
	let loadingApproveDialog = $state(false);
	let approveOpen = $state(false);
	let selectedPending = $state.raw<PendingExternalAuthView | undefined>(undefined);
	let approveOpener = $state<HTMLElement | null>(null);

	let revokeOpen = $state(false);
	let revokeOpener = $state<HTMLElement | null>(null);
	let currentTargetKey = '';
	let lastLoadKey = '';
	let destroyed = false;

	const membersById = $derived(buildMemberNameMap(snapshotMembers));
	const snapshotMember = $derived(snapshotMembers.find((m) => m.id === profile.id));
	const isBuiltIn = $derived(
		!canUseExternalSignIn(snapshotMember, {
			kind: profile.kind,
			handle: profile.handle,
			role: profile.role
		})
	);
	const activeLease = $derived(
		controller.data?.active?.active ? controller.data.active : undefined
	);
	const pendingList = $derived(controller.data?.pending ?? []);
	const historyList = $derived(controller.data?.history ?? []);
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
		const orgId = organizationId;
		const memId = profile.id;
		const rev = changes;
		const builtIn = isBuiltIn;
		const sessionKind = appSession.state.kind;
		const targetKey = `${orgId}:${memId}`;
		const targetChanged = targetKey !== currentTargetKey;
		if (builtIn) {
			currentTargetKey = targetKey;
			lastLoadKey = `builtin:${targetKey}`;
			untrack(() => {
				approveOpen = false;
				revokeOpen = false;
				selectedPending = undefined;
			});
			return;
		}
		if (sessionKind === 'loading') return;
		const loadKey = `${sessionKind}:${targetKey}:${rev}`;
		if (loadKey === lastLoadKey) return;
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
		if (isBuiltIn) return;
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
		const notice = formatApprovedSessionToast(session, {
			expectedEmployeeId: profile.id,
			expectedAgentName: profile.name,
			membersById
		});
		if (notice.kind === 'warning') {
			toast.warning(notice.title, { description: notice.description });
		} else {
			toast.success(notice.title, { description: notice.description });
		}
		await refreshAfterMutation();
	}

	function isCurrentTarget(targetOrg: string, targetMemberId: string): boolean {
		return !destroyed && organizationId === targetOrg && profile.id === targetMemberId;
	}

	function isAbortError(err: unknown): boolean {
		return err instanceof DOMException && err.name === 'AbortError';
	}

	async function handleRenew() {
		if (!activeLease || controller.busyAction || isBuiltIn) return;
		const targetOrg = organizationId;
		const targetMemberId = profile.id;
		const targetName = profile.name;
		try {
			const renewed = await controller.renew(
				targetOrg,
				targetMemberId,
				{
					session_id: activeLease.session_id,
					ttl_seconds: RENEW_7D_TTL_SECONDS
				},
				{ recordActionError: false }
			);
			if (!isCurrentTarget(targetOrg, targetMemberId)) return;
			nowMs = Date.now();
			toast.success(`Renewed external lease for ${targetName} (+7d).`, {
				description: `Expires ${formatExternalAuthTime(renewed.expires_at)}`
			});
		} catch (err) {
			if (
				!isCurrentTarget(targetOrg, targetMemberId) ||
				appSession.state.kind !== 'signed-in' ||
				isAbortError(err)
			) {
				return;
			}
			const parsed = asExternalAuthError(err, 'External lease renewal failed.');
			toast.error(`Couldn't renew external lease for ${targetName}.`, {
				description: formatExternalAuthErrorMessage(parsed)
			});
		}
	}

	async function confirmRevoke() {
		const targetOrg = organizationId;
		const targetMemberId = profile.id;
		const targetName = profile.name;
		const res = await controller.revoke(
			targetOrg,
			targetMemberId,
			{ ...(activeLease ? { session_id: activeLease.session_id } : {}) },
			{ reload: !onChanged, recordActionError: false }
		);
		if (!isCurrentTarget(targetOrg, targetMemberId)) return;
		nowMs = Date.now();
		toast.success(
			res.revoked
				? `Revoked external lease for ${targetName}.`
				: `No active external lease remained for ${targetName}.`
		);
		if (onChanged) await refreshAfterMutation();
	}
</script>

<div {@attach attachLoad} {@attach attachTicker} data-testid="external-auth-section">
	<Section
		level={3}
		title="External sign-in (BYOA)"
		description={isBuiltIn
			? undefined
			: 'Authorize external workspaces via agent-os login, monitor active external leases, renew for +7d, or revoke access immediately.'}
	>
		{#snippet actions()}
			{#if !isBuiltIn && controller.status !== 'unauthorized'}
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
						onclick={(e) => void openApproveFor(undefined, e.currentTarget)}
					>
						<KeyRound class="size-4" aria-hidden="true" />
						<span>Enter code…</span>
					</Button>
				{/if}
			{/if}
		{/snippet}

		{#if isBuiltIn}
			<p class="{gutters} text-sm/6 text-fg-muted" data-testid="external-auth-builtin-note">
				Built-in agents can't use external sign-in.
			</p>
		{:else if controller.status === 'idle' || controller.status === 'loading'}
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
								{membersById}
								busy={Boolean(controller.busyAction) || loadingApproveDialog}
								onApprove={(req, opener) => void openApproveFor(req, opener)}
							/>
						</div>
					{/if}

					<ExternalAuthActiveLeaseCard
						lease={activeLease}
						{nowMs}
						busyAction={controller.busyAction}
						{membersById}
						onRenew={() => void handleRenew()}
						onRevoke={(from) => {
							revokeOpener = from;
							revokeOpen = true;
						}}
					/>

					<ExternalAuthHistoryList history={historyList} {membersById} />
				{/if}
			</div>
		{/if}
	</Section>
</div>

{#if ApproveDialog && !isBuiltIn}
	<ApproveDialog
		bind:open={approveOpen}
		{organizationId}
		agentName={profile.name}
		agentHandle={profile.handle}
		expectedEmployeeId={profile.id}
		pending={selectedPending}
		pendingCandidates={pendingList}
		{membersById}
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
