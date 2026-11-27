<!--
  @component
  Dedicated decision actions for pending BYOA external-agent sign-in requests in Inbox / Needs-You
  (`TASK-249`, `doc-102` §4). Lazy-loads `ExternalAuthApproveDialog` on first open, refreshes shell
  data, and recovers focus cleanly when the approved item disappears from the Inbox queue.
-->
<script lang="ts">
	import { onDestroy, tick, untrack } from 'svelte';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import KeyRound from '@lucide/svelte/icons/key-round';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { tone } from '$lib/components/pills';
	import {
		familiarityInfo,
		formatExternalAuthTime,
		formatHandle,
		formatLastApproved,
		formatTimeRemaining,
		type ExternalSessionView,
		type FamiliaritySeverity,
		type PendingExternalAuthView
	} from '$lib/platform/workspace/external-auth';
	import type { InboxExternalAuthMeta } from '$lib/platform/workspace/inbox-types';
	import { useShellData } from '$lib/platform/workspace/shell-data-context';

	type ApproveDialogComponent =
		typeof import('$lib/modules/members/external-auth-ui').ExternalAuthApproveDialog;

	const FAMILIARITY_TONE: Record<FamiliaritySeverity, string> = {
		warning: tone.owner,
		caution: tone.bad,
		familiar: tone.ok,
		unknown: tone.neutral
	};

	let {
		meta,
		agentName,
		controlsDisabled = false,
		nowMs
	}: {
		meta: InboxExternalAuthMeta;
		agentName: string;
		controlsDisabled?: boolean;
		nowMs: number;
	} = $props();

	const shellData = useShellData();

	let ApproveDialog = $state.raw<ApproveDialogComponent | undefined>(undefined);
	let loadingDialog = $state(false);
	let dialogOpen = $state(false);
	let opener = $state<HTMLElement | null>(null);
	let currentRequestId = '';
	let destroyed = false;

	const pendingView = $derived<PendingExternalAuthView>({ ...meta });
	const familiarity = $derived(familiarityInfo(meta.workspace_familiarity));
	const familiarityTone = $derived(FAMILIARITY_TONE[familiarity.severity]);
	const lastApproved = $derived(formatLastApproved(meta.last_approved_by, meta.last_approved_at));
	const handleLabel = $derived(formatHandle(meta.handle));
	const remaining = $derived(formatTimeRemaining(meta.expires_at, nowMs));

	const btnTouch =
		'[@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11';

	onDestroy(() => {
		destroyed = true;
	});

	function attachRequestSync() {
		const requestId = meta.request_id;
		if (currentRequestId && currentRequestId !== requestId) {
			untrack(() => {
				dialogOpen = false;
			});
		}
		currentRequestId = requestId;
	}

	function isVisibleFocusable(el: HTMLElement | null | undefined): el is HTMLElement {
		return Boolean(el && el.isConnected && el.getClientRects().length > 0);
	}

	function inboxFocusFallback(): HTMLElement | null {
		if (typeof document === 'undefined') return null;
		const candidates = [
			document.getElementById('triage-detail-heading'),
			document.querySelector<HTMLElement>('[data-triage-row]'),
			document.getElementById('shell-page-title')
		];
		return candidates.find(isVisibleFocusable) ?? document.getElementById('shell-page-title');
	}

	function recoverFocusIfStranded() {
		if (typeof document === 'undefined' || !document.querySelector('.triage-workspace')) return;
		const active = document.activeElement;
		if (
			!active ||
			active === document.body ||
			active === document.documentElement ||
			!active.isConnected
		) {
			inboxFocusFallback()?.focus({ preventScroll: true });
		}
	}

	async function openApproveDialog(from: HTMLElement | null) {
		const startRequestId = meta.request_id;
		opener = from;
		if (!ApproveDialog) {
			if (loadingDialog) return;
			loadingDialog = true;
			try {
				const mod = await import('$lib/modules/members/external-auth-ui');
				if (destroyed || meta.request_id !== startRequestId) return;
				ApproveDialog = mod.ExternalAuthApproveDialog;
			} catch {
				toast.error('Could not open the approval dialog. Refresh and try again.');
				return;
			} finally {
				loadingDialog = false;
			}
		}
		if (!destroyed && meta.request_id === startRequestId) dialogOpen = true;
	}

	async function handleApproved(session: ExternalSessionView) {
		toast.success(`Approved external sign-in for ${agentName}.`, {
			description: `${session.workspace_label} · expires ${formatExternalAuthTime(session.expires_at)}`
		});
		await tick();
		try {
			const refreshed = await shellData.refresh();
			if (!refreshed.inboxUpdated) {
				toast.warning('Sign-in approved, but Inbox could not refresh automatically.');
				return;
			}
		} catch {
			toast.warning('Sign-in approved, but Inbox could not refresh automatically.');
			return;
		}
		await tick();
		recoverFocusIfStranded();
		setTimeout(recoverFocusIfStranded, 0);
	}
</script>

<div
	{@attach attachRequestSync}
	class="ext-auth-card w-full space-y-3"
	data-testid="inbox-external-auth-actions"
>
	<div class="rounded-md border border-line bg-tint/[0.03] p-3.5 text-xs/5">
		<div class="flex flex-wrap items-center gap-2">
			<StatusPill label={familiarity.label} class={familiarityTone}>
				{#snippet icon()}
					{#if familiarity.severity === 'familiar'}
						<ShieldCheck class="size-3.5" />
					{:else if familiarity.severity === 'caution'}
						<ShieldAlert class="size-3.5" />
					{:else}
						<AlertTriangle class="size-3.5" />
					{/if}
				{/snippet}
			</StatusPill>
			{#if meta.managed_turn_active}
				<StatusPill label="Managed turn active" class={tone.owner} />
			{/if}
			<span class="font-mono text-fg-secondary">Lookup {meta.code_prefix}</span>
			<span class="text-fg-muted">· {remaining}</span>
		</div>

		{#if lastApproved}
			<p class="mt-2 text-fg-secondary [overflow-wrap:anywhere]">{lastApproved}</p>
		{/if}

		<dl class="mt-2.5 grid grid-cols-1 gap-x-5 gap-y-1.5 text-fg-muted sm:grid-cols-2">
			<div class="min-w-0">
				<dt class="font-medium text-fg-secondary">Office</dt>
				<dd class="[overflow-wrap:anywhere]">{meta.office_name}</dd>
			</div>
			{#if meta.hostname}
				<div class="min-w-0">
					<dt class="font-medium text-fg-secondary">Host</dt>
					<dd class="font-mono [overflow-wrap:anywhere]">{meta.hostname}</dd>
				</div>
			{/if}
			<div class="min-w-0 sm:col-span-2">
				<dt class="font-medium text-fg-secondary">Workspace</dt>
				<dd class="font-mono [overflow-wrap:anywhere]">
					{meta.workspace_path
						? `${meta.workspace_label} (${meta.workspace_path})`
						: meta.workspace_label}
				</dd>
			</div>
			<div class="min-w-0 sm:col-span-2">
				<dt class="font-medium text-fg-secondary">Public key fingerprint</dt>
				<dd class="font-mono [overflow-wrap:anywhere]">{meta.pubkey_fingerprint}</dd>
			</div>
		</dl>
	</div>

	<div class="flex flex-wrap items-center gap-2">
		<button
			type="button"
			disabled={controlsDisabled || loadingDialog}
			onclick={(e) => void openApproveDialog(e.currentTarget)}
			class="exec-cta min-w-9 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 {btnTouch}"
		>
			<span class="exec-cta-fill" aria-hidden="true"></span>
			<span class="exec-cta-inner">
				<KeyRound class="size-4" aria-hidden="true" />
				<span>Approve sign-in…</span>
			</span>
		</button>

		<a
			href={resolve(`/members/${encodeURIComponent(meta.employee_id)}` as '/teams/members')}
			class="ext-auth-profile-link inline-flex min-h-9 min-w-9 items-center justify-center gap-1.5 px-3.5 text-xs font-semibold {btnTouch}"
		>
			<ExternalLink class="size-3.5 shrink-0" aria-hidden="true" />
			<span>Open {handleLabel ?? 'agent profile'}</span>
		</a>
	</div>
</div>

{#if ApproveDialog}
	<ApproveDialog
		bind:open={dialogOpen}
		organizationId={shellData.organizationId}
		{agentName}
		agentHandle={meta.handle}
		pending={pendingView}
		{opener}
		fallback={inboxFocusFallback}
		onApproved={handleApproved}
	/>
{/if}

<style>
	.ext-auth-profile-link {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		text-decoration: none;
		transition: background-color 120ms ease-out;
	}
	.ext-auth-profile-link:hover {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring-hover);
	}
	.ext-auth-profile-link:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	@media (forced-colors: active) {
		.ext-auth-profile-link {
			border: 1px solid ButtonText;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.ext-auth-profile-link {
			transition: none;
		}
	}
</style>
