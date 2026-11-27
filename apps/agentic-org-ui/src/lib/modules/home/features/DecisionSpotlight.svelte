<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import type { OrgSnapshot } from '$lib/proto';
	import { isDenyAction } from '$lib/modules/inbox/spotlight';
	import type { HomeInboxState, SpotlightDecision } from '$lib/modules/home/model';
	import { useShellAi } from '$lib/platform/workspace/shell-ai';
	import HomeDayOne from './HomeDayOne.svelte';
	import { SpotlightController } from './spotlight-controller.svelte';
	import SpotlightActionBar from './SpotlightActionBar.svelte';
	import SpotlightContextPanel from './SpotlightContextPanel.svelte';
	import SpotlightHeaderBar from './SpotlightHeaderBar.svelte';
	import SpotlightRecommendation from './SpotlightRecommendation.svelte';
	import SpotlightRiskStrip from './SpotlightRiskStrip.svelte';
	import SpotlightSessionReceipt from './SpotlightSessionReceipt.svelte';

	interface Props {
		spotlight: readonly SpotlightDecision[];
		organizationId: string;
		snapshot: OrgSnapshot;
		isDayOne: boolean;
		inboxState?: HomeInboxState;
		ariaLabelledby?: string;
		onRefresh: () => Promise<void>;
	}

	let {
		spotlight,
		organizationId,
		snapshot,
		isDayOne,
		inboxState = 'ready',
		ariaLabelledby,
		onRefresh
	}: Props = $props();

	const shellAi = useShellAi();
	const controller = new SpotlightController({
		getOrgId: () => organizationId,
		navigate: (href) => goto(resolve(href as '/work')),
		notifySuccess: (msg) => {
			toast.success(msg);
		},
		notifyError: (msg) => {
			toast.error(msg);
		},
		refresh: () => onRefresh()
	});

	let spotIdx = $derived.by(() => {
		void organizationId;
		return 0;
	});
	let decliningId = $derived.by<string | undefined>(() => {
		void organizationId;
		return undefined;
	});
	let declineReason = $derived.by(() => {
		void organizationId;
		return '';
	});
	let replyingId = $derived.by<string | undefined>(() => {
		void organizationId;
		return undefined;
	});
	let replyText = $derived.by(() => {
		void organizationId;
		return '';
	});
	let restoreDenyFocus = $state(false);
	let restoreReplyFocus = $state(false);
	let openContextIds = $derived.by<Record<string, boolean>>(() => {
		void organizationId;
		return {};
	});
	let pulsing = $state(false);
	let pulseTimer: ReturnType<typeof setTimeout> | undefined;
	let panelEl = $state<HTMLElement | null>(null);
	let titleEl = $state<HTMLHeadingElement | null>(null);
	onDestroy(() => {
		controller.dispose();
		clearTimeout(pulseTimer);
	});

	export function jumpToDecision(id: string): boolean {
		const idx = spotlight.findIndex((item) => item.id === id);
		if (idx < 0) return false;
		spotIdx = idx;
		decliningId = undefined;
		declineReason = '';
		replyingId = undefined;
		replyText = '';
		pulsing = true;
		clearTimeout(pulseTimer);
		pulseTimer = setTimeout(() => {
			pulsing = false;
		}, 950);
		void tick().then(() => {
			const prefersReduced =
				typeof window !== 'undefined' &&
				window.matchMedia('(prefers-reduced-motion: reduce)').matches;
			panelEl?.scrollIntoView({
				behavior: prefersReduced ? 'auto' : 'smooth',
				block: 'center'
			});
			titleEl?.focus({ preventScroll: true });
		});
		return true;
	}

	const safeIdx = $derived(spotlight.length > 0 ? Math.min(spotIdx, spotlight.length - 1) : 0);
	const currentDecision = $derived(spotlight[safeIdx]);
	const handledIds = $derived(new Set(Object.keys(controller.handledReceipts)));
	const currentReceipt = $derived(
		currentDecision ? controller.handledReceipts[currentDecision.id] : undefined
	);
	const currentCtxOpen = $derived(
		currentDecision ? Boolean(openContextIds[currentDecision.id]) : false
	);
	const isCurrentBusy = $derived(controller.isBusyFor(currentDecision?.id));

	function selectDecision(index: number) {
		spotIdx = index;
		decliningId = undefined;
		declineReason = '';
		replyingId = undefined;
		replyText = '';
	}

	function armDeny(id: string) {
		restoreDenyFocus = false;
		decliningId = id;
		declineReason = '';
		replyingId = undefined;
		replyText = '';
	}

	function cancelDeny() {
		restoreDenyFocus = true;
		decliningId = undefined;
		declineReason = '';
	}

	function openReply(id: string) {
		restoreReplyFocus = false;
		decliningId = undefined;
		declineReason = '';
		replyingId = id;
		replyText = '';
	}

	function cancelReply() {
		restoreReplyFocus = true;
		replyingId = undefined;
		replyText = '';
	}

	function toggleContext(id: string) {
		openContextIds = { ...openContextIds, [id]: !openContextIds[id] };
	}

	async function focusAfterHandle() {
		await tick();
		if (controller.destroyed) return;
		const receiptEl = panelEl?.querySelector<HTMLElement>('[data-spotlight-receipt]');
		const target = receiptEl ?? titleEl;
		target?.focus({ preventScroll: true });
	}

	function askAboutDecision(decision: SpotlightDecision) {
		shellAi?.ask(`Explain the risk, impact, and recommendation for: ${decision.title}`, {
			decisionId: decision.id,
			decision: {
				id: decision.id,
				title: decision.title,
				recommendation: decision.recommendation,
				risk: decision.risk,
				impact: decision.impact,
				backgroundContext: decision.backgroundContext,
				detailHref: decision.detailHref,
				projectId: decision.projectId,
				projectName: decision.projectName
			}
		});
	}

	async function submitReply() {
		const decision = currentDecision;
		if (!decision) return;
		const handled = await controller.submitReply(decision, replyText, () => {
			replyingId = undefined;
			replyText = '';
		});
		if (handled) await focusAfterHandle();
	}

	async function runAction(kind: 'primary' | 'secondary') {
		const decision = currentDecision;
		if (!decision || controller.isOrgBusy()) return;
		if (
			kind === 'secondary' &&
			isDenyAction(decision.secondaryAction) &&
			decliningId !== decision.id
		) {
			armDeny(decision.id);
			return;
		}
		const handled = await controller.runAction(decision, kind, snapshot, declineReason, () => {
			decliningId = undefined;
			declineReason = '';
		});
		if (handled) await focusAfterHandle();
	}
</script>

<article
	bind:this={panelEl}
	id="hb-spotlight"
	class={[
		'spot-panel relative flex flex-col gap-4 overflow-hidden',
		currentDecision?.urgent && !currentReceipt && 'is-urgent',
		(Boolean(currentReceipt) || !currentDecision) && 'is-done',
		isDayOne && 'is-empty-hero',
		pulsing && 'is-pulse'
	]}
	aria-labelledby={ariaLabelledby}
	aria-label={ariaLabelledby ? undefined : isDayOne ? 'Get started' : 'Decision spotlight'}
>
	{#if currentDecision}
		<SpotlightHeaderBar
			{currentDecision}
			{spotlight}
			{safeIdx}
			{handledIds}
			onSelect={selectDecision}
		/>

		{#if controller.lastReceipt && !currentReceipt}
			<SpotlightSessionReceipt
				title={controller.lastReceipt.title}
				outcome={controller.lastReceipt.outcome}
				note={controller.lastReceipt.note}
				onDismiss={() => controller.dismissLastReceipt()}
			/>
		{/if}

		<div class="spot-main flex flex-col gap-3.5">
			<div class="flex flex-wrap items-center gap-2 text-xs text-[var(--exec-muted)]">
				<span
					class="avatar-chip inline-flex size-6 items-center justify-center rounded-full text-[10px] font-bold text-[var(--exec-ink)]"
				>
					{currentDecision.actorInitials}
				</span>
				{#if currentDecision.actorId}
					<a
						href={resolve(
							`/members/${encodeURIComponent(currentDecision.actorId)}` as '/teams/members'
						)}
						class="spot-meta-link inline-flex min-h-9 items-center font-semibold text-[var(--exec-ink)] hover:underline"
					>
						{currentDecision.actorName}
					</a>
				{:else}
					<span class="font-semibold text-[var(--exec-ink)]">{currentDecision.actorName}</span>
				{/if}
				<span aria-hidden="true">·</span>
				<span>{currentDecision.kindLabel}</span>
				<span aria-hidden="true">·</span>
				<span>{currentDecision.ageLabel}</span>
				<span aria-hidden="true">·</span>
				{#if currentDecision.projectId}
					<a
						href={resolve(
							`/projects/${encodeURIComponent(currentDecision.projectId)}/work` as '/projects'
						)}
						class="spot-meta-link inline-flex min-h-9 items-center hover:text-[var(--exec-ink)] hover:underline"
					>
						{currentDecision.projectName}
					</a>
				{:else}
					<span>{currentDecision.projectName}</span>
				{/if}
			</div>

			<h3 bind:this={titleEl} tabindex="-1" class="spot-title focus:outline-none">
				{currentDecision.title}
			</h3>

			{#if !currentReceipt}
				<SpotlightRecommendation
					tone={currentDecision.recTone}
					riskLabel={currentDecision.recRiskLabel}
					recommendation={currentDecision.recommendation}
				/>

				<SpotlightRiskStrip
					cells={[currentDecision.risk, currentDecision.impact, currentDecision.proof]}
				/>
			{/if}

			<SpotlightContextPanel
				decisionId={currentDecision.id}
				backgroundContext={currentDecision.backgroundContext}
				auditChecks={currentDecision.auditChecks}
				open={currentCtxOpen}
				onToggle={() => toggleContext(currentDecision.id)}
				onAsk={() => askAboutDecision(currentDecision)}
			/>
		</div>

		<SpotlightActionBar
			decision={currentDecision}
			totalCount={spotlight.length}
			busy={isCurrentBusy}
			receipt={currentReceipt}
			declining={decliningId === currentDecision.id}
			bind:declineReason
			replying={replyingId === currentDecision.id}
			bind:replyText
			{restoreDenyFocus}
			onClearRestoreDenyFocus={() => (restoreDenyFocus = false)}
			{restoreReplyFocus}
			onClearRestoreReplyFocus={() => (restoreReplyFocus = false)}
			onPrimary={() => void runAction('primary')}
			onSecondary={() => void runAction('secondary')}
			onArmDeny={() => armDeny(currentDecision.id)}
			onCancelDeny={cancelDeny}
			onOpenReply={() => openReply(currentDecision.id)}
			onSubmitReply={() => void submitReply()}
			onCancelReply={cancelReply}
		/>
	{:else}
		<div class="my-auto flex flex-col justify-center">
			<HomeDayOne
				{isDayOne}
				{inboxState}
				nested={Boolean(ariaLabelledby)}
				sessionHandledCount={controller.sessionHandledCount}
				lastReceipt={controller.lastReceipt}
			/>
		</div>
	{/if}
</article>

<style>
	.spot-panel {
		min-height: 280px;
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: var(--exec-spot-pad-sm);
		transition: box-shadow 0.28s ease;
	}
	.spot-panel.is-empty-hero {
		min-height: 248px;
	}
	.spot-panel:hover {
		background: var(--exec-panel-bg-hover);
		box-shadow: var(--exec-panel-shadow-hover);
	}
	.spot-panel.is-pulse {
		box-shadow:
			inset 0 0 0 1.5px var(--tone-accent),
			var(--exec-panel-shadow-hover);
	}
	@media (min-width: 640px) {
		.spot-panel {
			padding: var(--exec-spot-pad-md);
		}
	}
	@media (min-width: 1280px) {
		.spot-panel {
			padding: var(--exec-spot-pad-xl);
		}
	}
	.spot-panel::before {
		content: '';
		display: var(--exec-spot-accent-display);
		position: absolute;
		left: 0;
		right: 0;
		top: 0;
		height: 2px;
		background: linear-gradient(90deg, var(--exec-accent) 0%, transparent 60%);
	}
	.spot-panel.is-urgent::before {
		background: linear-gradient(90deg, var(--tone-warn) 0%, transparent 60%);
	}
	.spot-panel.is-done::before {
		background: linear-gradient(90deg, var(--tone-ok) 0%, transparent 60%);
	}
	.spot-title {
		font-size: var(--exec-spot-title-size);
		line-height: 1.3;
		font-weight: var(--exec-spot-title-weight);
		letter-spacing: -0.02em;
		color: var(--exec-ink);
		max-width: 48ch;
	}
	.avatar-chip {
		background: var(--exec-avatar-bg);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.spot-meta-link {
			min-height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.spot-panel {
			transition: none;
		}
	}
</style>
