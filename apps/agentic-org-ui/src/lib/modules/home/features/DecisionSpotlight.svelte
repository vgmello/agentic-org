<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import type { Attachment } from 'svelte/attachments';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Ban from '@lucide/svelte/icons/ban';
	import Check from '@lucide/svelte/icons/check';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clock from '@lucide/svelte/icons/clock';
	import Info from '@lucide/svelte/icons/info';
	import Scale from '@lucide/svelte/icons/scale';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Zap from '@lucide/svelte/icons/zap';
	import type { OrgSnapshot } from '$lib/proto';
	import { executeDecisionAction } from '$lib/home/actions';
	import { isDenyAction, type HomeInboxState, type SpotlightDecision } from '$lib/home/model';
	import { toAppError } from '$lib/proto';
	import { useShellAi } from '$lib/platform/shell/shell-ai';
	import HomeDayOne from './HomeDayOne.svelte';

	interface Props {
		spotlight: readonly SpotlightDecision[];
		organizationId: string;
		snapshot: OrgSnapshot;
		isDayOne: boolean;
		inboxState?: HomeInboxState;
		onRefresh: () => Promise<void>;
	}

	let {
		spotlight,
		organizationId,
		snapshot,
		isDayOne,
		inboxState = 'ready',
		onRefresh
	}: Props = $props();

	const shellAi = useShellAi();

	let spotIdx = $derived.by(() => {
		void organizationId;
		return 0;
	});
	let busyId = $state<string | undefined>(undefined);
	let decliningId = $derived.by<string | undefined>(() => {
		void organizationId;
		return undefined;
	});
	let declineReason = $derived.by(() => {
		void organizationId;
		return '';
	});
	let restoreDenyFocus = false;
	let openContextIds = $derived.by<Record<string, boolean>>(() => {
		void organizationId;
		return {};
	});
	let pulsing = $state(false);
	let pulseTimer: ReturnType<typeof setTimeout> | undefined;
	let panelEl = $state<HTMLElement | null>(null);
	let titleEl = $state<HTMLHeadingElement | null>(null);

	onDestroy(() => {
		clearTimeout(pulseTimer);
	});

	export function jumpToDecision(id: string): boolean {
		const idx = spotlight.findIndex((item) => item.id === id);
		if (idx < 0) return false;
		spotIdx = idx;
		decliningId = undefined;
		declineReason = '';
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

	const focusDenyInput: Attachment<HTMLInputElement> = (el) => {
		el.focus();
	};

	const focusDenyTriggerOnCancel: Attachment<HTMLButtonElement> = (el) => {
		if (restoreDenyFocus) {
			restoreDenyFocus = false;
			el.focus();
		}
	};

	const safeIdx = $derived(spotlight.length > 0 ? Math.min(spotIdx, spotlight.length - 1) : 0);
	const visibleDots = $derived(spotlight.slice(0, 6));
	const currentDecision = $derived(spotlight[safeIdx]);
	const nextDecision = $derived(
		spotlight.length > 1 ? spotlight[(safeIdx + 1) % spotlight.length] : undefined
	);
	const currentCtxOpen = $derived(
		currentDecision ? Boolean(openContextIds[currentDecision.id]) : false
	);

	function selectDecision(index: number) {
		spotIdx = index;
		decliningId = undefined;
		declineReason = '';
	}

	function armDeny(id: string) {
		restoreDenyFocus = false;
		decliningId = id;
		declineReason = '';
	}

	function cancelDeny() {
		restoreDenyFocus = true;
		decliningId = undefined;
		declineReason = '';
	}

	function handleDenyKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter') {
			event.preventDefault();
			void runAction('secondary');
		} else if (event.key === 'Escape') {
			event.preventDefault();
			cancelDeny();
		}
	}

	function toggleContext(id: string) {
		openContextIds = { ...openContextIds, [id]: !openContextIds[id] };
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

	async function runAction(kind: 'primary' | 'secondary') {
		if (!currentDecision || busyId) return;
		if (
			kind === 'secondary' &&
			isDenyAction(currentDecision.secondaryAction) &&
			decliningId !== currentDecision.id
		) {
			armDeny(currentDecision.id);
			return;
		}
		busyId = currentDecision.id;
		try {
			const res = await executeDecisionAction(
				organizationId,
				currentDecision,
				kind,
				snapshot,
				declineReason
			);
			decliningId = undefined;
			declineReason = '';
			if (res.navigateTo) {
				await goto(resolve(res.navigateTo as '/work'));
				return;
			}
			if (res.message) toast.success(res.message);
			await onRefresh();
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			busyId = undefined;
		}
	}
</script>

<article
	bind:this={panelEl}
	id="hb-spotlight"
	class={[
		'spot-panel relative flex min-h-[280px] flex-col gap-4 overflow-hidden',
		currentDecision?.urgent && 'is-urgent',
		pulsing && 'is-pulse'
	]}
	aria-label="Decision Spotlight"
>
	<div class="section-head items-baseline justify-between gap-3">
		<h2 class="text-lg font-bold tracking-tight text-[var(--exec-ink)]">
			Decision &amp; Approval Queue
		</h2>
	</div>

	{#if currentDecision}
		<div class="flex flex-wrap items-center justify-between gap-2">
			<span
				class={[
					'spot-badge inline-flex items-center gap-1.5',
					currentDecision.urgent && 'is-urgent'
				]}
			>
				<Clock class="size-3.5" aria-hidden="true" />
				<span>{currentDecision.badgeText}</span>
			</span>
			<div class="flex flex-wrap items-center gap-2.5">
				<div class="queue-dots flex items-center gap-1.5" aria-hidden="true">
					{#each visibleDots as d, i (d.id)}
						<span class={['queue-dot', i === safeIdx && 'is-cur', d.urgent && 'is-urgent']}></span>
					{/each}
				</div>
				<span class="text-xs font-semibold text-[var(--exec-muted)] tabular-nums">
					Priority {safeIdx + 1} of {spotlight.length}
				</span>
				<div class="flex items-center gap-1">
					<button
						type="button"
						disabled={spotlight.length <= 1}
						onclick={() => selectDecision((safeIdx - 1 + spotlight.length) % spotlight.length)}
						aria-label="Previous decision"
						class="stepper-arrow flex size-9 items-center justify-center rounded-full"
					>
						<ChevronLeft class="size-4" aria-hidden="true" />
					</button>
					<button
						type="button"
						disabled={spotlight.length <= 1}
						onclick={() => selectDecision((safeIdx + 1) % spotlight.length)}
						aria-label="Next decision"
						class="stepper-arrow flex size-9 items-center justify-center rounded-full"
					>
						<ChevronRight class="size-4" aria-hidden="true" />
					</button>
				</div>
			</div>
		</div>

		<div class="flex flex-1 flex-col gap-3.5">
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

			<div
				class={[
					'rec-callout flex items-start gap-2.5 px-3.5 py-2.5 text-sm',
					`tone-${currentDecision.recTone}`
				]}
				data-tone={currentDecision.recTone}
			>
				{#if currentDecision.recTone === 'ok'}
					<CheckCircle2 class="rec-ico mt-0.5 size-4 shrink-0" aria-hidden="true" />
				{:else if currentDecision.recTone === 'warn'}
					<AlertTriangle class="rec-ico mt-0.5 size-4 shrink-0" aria-hidden="true" />
				{:else if currentDecision.recTone === 'bad'}
					<ShieldAlert class="rec-ico mt-0.5 size-4 shrink-0" aria-hidden="true" />
				{:else}
					<Info class="rec-ico mt-0.5 size-4 shrink-0" aria-hidden="true" />
				{/if}
				<div class="min-w-0 flex-1">
					{#if currentDecision.recRiskLabel}
						<p class="rec-lbl mb-0.5 text-xs font-semibold tracking-wider uppercase">
							Recommendation · {currentDecision.recRiskLabel}
						</p>
					{/if}
					<p class="leading-relaxed">{currentDecision.recommendation}</p>
				</div>
			</div>

			<!-- 3-Column Risk, Impact & Proof Strip -->
			<div class="exec-well grid grid-cols-1 gap-2.5 p-3 sm:grid-cols-3 sm:gap-0 sm:px-1">
				{#each [currentDecision.risk, currentDecision.impact, currentDecision.proof] as cell, ci (cell.label + ci)}
					<div class={['flex min-w-0 items-center gap-2.5 sm:px-3', ci > 0 && 'cell-sep']}>
						<span
							class={[
								'well-ico flex size-8 shrink-0 items-center justify-center rounded-full',
								`tone-${cell.tone}`
							]}
						>
							{#if cell.icon === 'shield-check'}
								<ShieldCheck class="size-4" aria-hidden="true" />
							{:else if cell.icon === 'shield-alert'}
								<ShieldAlert class="size-4" aria-hidden="true" />
							{:else if cell.icon === 'bolt'}
								<Zap class="size-4" aria-hidden="true" />
							{:else if cell.icon === 'scale'}
								<Scale class="size-4" aria-hidden="true" />
							{:else if cell.icon === 'check-circle'}
								<CheckCircle2 class="size-4" aria-hidden="true" />
							{:else}
								<Clock class="size-4" aria-hidden="true" />
							{/if}
						</span>
						<div class="min-w-0">
							<div class="truncate text-xs font-semibold text-[var(--exec-ink)]">
								{cell.label}
							</div>
							<div class="truncate text-xs text-[var(--exec-muted)]">
								{cell.sub}
							</div>
						</div>
					</div>
				{/each}
			</div>

			<!-- Inline expandable background context disclosure + Ask about this -->
			<div class="flex flex-col gap-2">
				<div class="spot-ctx-row flex flex-wrap items-center justify-between gap-2.5">
					<button
						type="button"
						onclick={() => toggleContext(currentDecision.id)}
						aria-expanded={currentCtxOpen}
						aria-controls="panel-spot-ctx-{currentDecision.id}"
						class="ctx-toggle inline-flex w-fit items-center gap-1.5 rounded-md px-1.5 text-xs font-semibold"
					>
						<ChevronRight
							class={['size-3.5 transition-transform', currentCtxOpen && 'rotate-90']}
							aria-hidden="true"
						/>
						<span>{currentCtxOpen ? 'Hide background context' : 'Read background context'}</span>
					</button>
					<button
						type="button"
						onclick={() => askAboutDecision(currentDecision)}
						class="spot-ai-ask inline-flex min-h-9 items-center gap-1.5 px-2.5 text-xs font-semibold"
					>
						<Sparkles class="size-3.5" aria-hidden="true" />
						<span>Ask about this</span>
					</button>
				</div>
				{#if currentCtxOpen}
					<div
						id="panel-spot-ctx-{currentDecision.id}"
						class="ctx-body px-3.5 py-2.5 text-xs leading-relaxed"
					>
						<p>{currentDecision.backgroundContext}</p>
						{#if currentDecision.auditChecks.length > 0}
							<div
								class="ctx-checks mt-2.5 flex flex-wrap gap-x-3.5 gap-y-1.5 pt-2.5"
								role="list"
								aria-label="Decision checks"
							>
								{#each currentDecision.auditChecks as chk (chk)}
									<span class="inline-flex items-center gap-1.5" role="listitem">
										<CheckCircle2
											class="size-3.5 shrink-0 text-[var(--exec-ok)]"
											aria-hidden="true"
										/>
										<span>{chk}</span>
									</span>
								{/each}
							</div>
						{/if}
					</div>
				{/if}
			</div>
		</div>

		<!-- Action Bar directly above UP NEXT -->
		<div class="spot-foot flex flex-wrap items-center justify-between gap-3 pt-3.5">
			<div class="flex flex-1 flex-wrap items-center gap-2">
				<button
					type="button"
					disabled={busyId === currentDecision.id}
					onclick={() => void runAction('primary')}
					class="exec-cta min-h-9"
				>
					<span class="exec-cta-fill" aria-hidden="true"></span>
					<span class="exec-cta-sheen" aria-hidden="true"></span>
					<span class="exec-cta-inner">
						<Check class="size-4" aria-hidden="true" />
						<span>{currentDecision.primaryAction.label}</span>
					</span>
				</button>
				{#if isDenyAction(currentDecision.secondaryAction)}
					{#if decliningId !== currentDecision.id}
						<span class="action-sep" aria-hidden="true"></span>
						<button
							{@attach focusDenyTriggerOnCancel}
							type="button"
							disabled={busyId === currentDecision.id}
							onclick={() => armDeny(currentDecision.id)}
							class="deny-btn inline-flex min-h-9 items-center gap-1.5 px-3 text-xs font-semibold"
						>
							<Ban class="size-3.5" aria-hidden="true" />
							<span>{currentDecision.secondaryAction.label}</span>
						</button>
					{/if}
				{:else if currentDecision.secondaryAction}
					<button
						type="button"
						disabled={busyId === currentDecision.id}
						onclick={() => void runAction('secondary')}
						class="outline-btn inline-flex min-h-9 items-center gap-1.5 px-3 text-xs font-semibold"
					>
						<span>{currentDecision.secondaryAction.label}</span>
					</button>
				{/if}
			</div>
			<div class="flex items-center gap-2">
				<a
					href={resolve(currentDecision.detailHref as '/work')}
					class="skip-btn inline-flex min-h-9 items-center gap-0.5 rounded-lg px-2 text-xs font-medium"
				>
					<span>Details</span>
					<ChevronRight class="size-3.5" aria-hidden="true" />
				</a>
				<a
					href={resolve('/inbox')}
					class="skip-btn inline-flex min-h-9 items-center gap-0.5 rounded-lg px-2 text-xs font-medium"
				>
					<span>All {spotlight.length} in Inbox</span>
					<ChevronRight class="size-3.5" aria-hidden="true" />
				</a>
			</div>

			{#if decliningId === currentDecision.id}
				<div
					class="deny-guard flex w-full flex-col gap-2 p-3"
					role="group"
					aria-label="Confirm deny"
				>
					<div class="flex flex-wrap items-center justify-between gap-2.5">
						<span class="deny-guard-lbl inline-flex items-center gap-1.5 text-xs font-semibold">
							<Ban class="size-3.5 shrink-0" aria-hidden="true" />
							<span>Confirm deny?</span>
						</span>
						<div class="flex items-center gap-1.5">
							<button
								type="button"
								disabled={busyId === currentDecision.id}
								onclick={() => void runAction('secondary')}
								class="deny-confirm inline-flex min-h-9 items-center rounded-md px-3 text-xs font-semibold"
							>
								Confirm
							</button>
							<button
								type="button"
								onclick={cancelDeny}
								class="skip-btn inline-flex min-h-9 items-center rounded-md px-2.5 text-xs font-semibold"
							>
								Keep
							</button>
						</div>
					</div>
					<label class="sr-only" for="deny-msg-{currentDecision.id}">
						Optional reason for {currentDecision.actorName}
					</label>
					<input
						{@attach focusDenyInput}
						bind:value={declineReason}
						onkeydown={handleDenyKeydown}
						id="deny-msg-{currentDecision.id}"
						type="text"
						autocomplete="off"
						placeholder="Optional reason for {currentDecision.actorName}… (Enter to confirm, Esc to cancel)"
						class="deny-input min-h-9 w-full rounded-md px-2.5 py-1.5 text-xs"
					/>
				</div>
			{/if}
		</div>

		<!-- UP NEXT tucked below action bar -->
		{#if nextDecision}
			<button
				type="button"
				onclick={() => selectDecision((safeIdx + 1) % spotlight.length)}
				class="exec-well next-bar flex min-h-9 items-center justify-between gap-2 px-3 py-2 text-left text-xs"
			>
				<span class="flex min-w-0 items-center gap-2">
					<span class="shrink-0 text-xs font-bold tracking-wider text-[var(--exec-dim)] uppercase">
						Up next
					</span>
					<span class="truncate font-medium text-[var(--exec-body)]">
						{nextDecision.title}
					</span>
				</span>
				<span class="flex shrink-0 items-center gap-2">
					{#if nextDecision.expiresShort}
						<span class="next-exp inline-flex items-center gap-1">
							<Clock class="size-3" aria-hidden="true" />
							<span>{nextDecision.expiresShort}</span>
						</span>
					{:else if nextDecision.isChiefOfStaff}
						<span class="next-ai inline-flex items-center gap-1">
							<Sparkles class="size-3" aria-hidden="true" />
							<span>{nextDecision.actorName}</span>
						</span>
					{/if}
					<ChevronRight class="size-3.5 shrink-0 text-[var(--exec-dim)]" aria-hidden="true" />
				</span>
			</button>
		{/if}
	{:else}
		<HomeDayOne {isDayOne} {inboxState} />
	{/if}
</article>

<style>
	.spot-panel {
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: var(--exec-spot-pad-sm);
		transition: box-shadow 0.28s ease;
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
	.section-head {
		display: var(--exec-section-head-display);
	}
	.spot-title {
		font-size: var(--exec-spot-title-size);
		line-height: 1.3;
		font-weight: var(--exec-spot-title-weight);
		letter-spacing: -0.02em;
		color: var(--exec-ink);
		max-width: 48ch;
	}
	.spot-badge {
		padding: 4px 11px;
		border-radius: 999px;
		font-size: 0.75rem;
		font-weight: 600;
		background: var(--exec-sky-bg);
		color: var(--exec-sky);
		box-shadow: inset 0 0 0 1px var(--exec-sky-ring);
	}
	.spot-badge.is-urgent {
		background: var(--exec-warn-bg);
		color: var(--exec-warn);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
	}
	.queue-dot {
		display: inline-block;
		width: 8px;
		height: 8px;
		border-radius: 999px;
		background: var(--field-border);
		transition:
			width 0.2s ease,
			background-color 0.16s ease;
	}
	.queue-dot.is-urgent {
		background: var(--tone-warn-ink);
	}
	.queue-dot.is-cur {
		width: 22px;
		background: var(--tone-accent-ink);
	}
	.queue-dot.is-cur.is-urgent {
		background: var(--tone-warn-ink);
	}
	.stepper-arrow {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		color: var(--exec-body);
	}
	.stepper-arrow:disabled {
		opacity: 0.35;
	}
	.avatar-chip {
		background: var(--exec-avatar-bg);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.rec-callout {
		--rec-ink: var(--exec-accent-ink);
		--rec-bar: var(--tone-accent);
		--rec-bg: var(--exec-accent-soft);
		--rec-ring: var(--exec-accent-ring);
		border-radius: var(--exec-inset-radius);
		background: var(--rec-bg);
		box-shadow:
			inset 0 0 0 1px var(--rec-ring),
			inset 3px 0 0 var(--rec-bar);
		color: var(--exec-body);
	}
	.rec-callout.tone-ok {
		--rec-ink: var(--exec-ok);
		--rec-bar: var(--tone-ok);
		--rec-bg: var(--exec-ok-bg);
		--rec-ring: var(--exec-ok-ring);
	}
	.rec-callout.tone-warn {
		--rec-ink: var(--exec-warn);
		--rec-bar: var(--tone-warn);
		--rec-bg: var(--exec-warn-bg);
		--rec-ring: var(--exec-warn-ring);
	}
	.rec-callout.tone-bad {
		--rec-ink: var(--exec-bad);
		--rec-bar: var(--tone-bad);
		--rec-bg: var(--exec-bad-bg);
		--rec-ring: var(--exec-bad-ring);
	}
	.rec-callout :global(.rec-ico),
	.rec-lbl {
		color: var(--rec-ink);
	}
	.cell-sep {
		border-top: 1px solid var(--exec-border);
		padding-top: 10px;
	}
	@media (min-width: 640px) {
		.cell-sep {
			border-top: 0;
			padding-top: 0;
			border-left: 1px solid var(--exec-border);
		}
	}
	.well-ico.tone-ok {
		background: var(--exec-ok-bg);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
		color: var(--exec-ok);
	}
	.well-ico.tone-warn {
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
		color: var(--exec-warn);
	}
	.well-ico.tone-sky {
		background: var(--exec-sky-bg);
		box-shadow: inset 0 0 0 1px var(--exec-sky-ring);
		color: var(--exec-sky);
	}
	.well-ico.tone-muted {
		background: var(--exec-surface);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		color: var(--exec-muted);
	}
	.ctx-toggle {
		min-height: 36px;
		color: var(--exec-accent-ink);
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.ctx-toggle:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.spot-ai-ask {
		border-radius: var(--exec-sm-radius);
		color: var(--exec-accent-ink);
		background: var(--exec-accent-soft);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.spot-ai-ask:hover {
		background: var(--exec-accent);
		color: var(--exec-on-accent);
		box-shadow: inset 0 0 0 1px var(--exec-accent);
	}
	.ctx-body {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-well);
		color: var(--exec-body);
		box-shadow: inset 2px 0 0 var(--tone-accent);
	}
	.ctx-checks {
		border-top: 1px solid var(--exec-border);
		color: var(--exec-muted);
	}
	.spot-foot {
		border-top: 1px solid var(--exec-border);
	}
	.action-sep {
		width: 1px;
		height: 20px;
		background: var(--exec-border-hi);
		margin: 0 2px;
		flex: none;
	}
	.outline-btn {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.outline-btn:hover {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring-hover);
	}
	.deny-btn {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-deny-bg);
		color: var(--exec-deny-ink);
		box-shadow: inset 0 0 0 1px var(--exec-deny-ring);
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.deny-btn:hover {
		background: var(--exec-deny-bg-hover);
		color: var(--exec-deny-ink-hover);
		box-shadow: inset 0 0 0 1px var(--exec-deny-ring-hover);
	}
	.deny-guard {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-deny-guard-bg);
		box-shadow: inset 0 0 0 1px var(--exec-deny-guard-ring);
	}
	.deny-guard-lbl {
		color: var(--exec-deny-ink);
	}
	.deny-confirm {
		background: var(--exec-deny-confirm-bg);
		color: var(--exec-deny-confirm-ink);
	}
	.deny-confirm:hover {
		background: var(--exec-deny-confirm-hover);
	}
	.skip-btn {
		color: var(--exec-muted);
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.skip-btn:hover {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.deny-input {
		background: var(--exec-surface);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.deny-input::placeholder {
		color: var(--exec-muted);
		opacity: 1;
	}
	.next-bar:hover {
		background: var(--exec-surface-hi);
	}
	.next-exp {
		padding: 2px 7px;
		border-radius: var(--exec-pill-radius);
		background: var(--exec-warn-bg);
		color: var(--exec-warn);
		box-shadow: var(--exec-pill-ring-warn);
		font-size: 0.6875rem;
		font-weight: 600;
		white-space: nowrap;
	}
	.next-ai {
		padding: 2px 7px;
		border-radius: var(--exec-pill-radius);
		background: var(--exec-accent-soft);
		color: var(--exec-accent-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
		font-size: 0.6875rem;
		font-weight: 600;
		white-space: nowrap;
	}
	@media (max-width: 720px), (pointer: coarse) {
		.deny-btn,
		.outline-btn,
		.skip-btn,
		.deny-confirm,
		.ctx-toggle,
		.spot-ai-ask,
		.spot-meta-link,
		.next-bar,
		.deny-input {
			min-height: 44px;
		}
		.stepper-arrow {
			min-width: 44px;
			min-height: 44px;
		}
	}
</style>
