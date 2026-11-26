<!--
  @component
  Zero-Reflow Bottom-Right Floating Tess Companion (TASK-145 / TASK-80.5 / doc-97).
  - Rests at bottom-right as a compact floating pill ([✦ Tess ⌘/]) with zero page reflow.
  - Opens upward on click or ⌘/ as a compact floating card growing up to 560px with conversation.
  - Maximize (⤢ or ⌘⇧E / ⌘⇧M) expands to a centered 680px Studio dialog with GPU-composited transitions.
  - Shows live refining indicator when async Tess task intakes run in Notifications.
  - Preserves Svelte's 1-click "Chat" DM handoff alongside honest local-summary badging.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import type { Attachment } from 'svelte/attachments';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import ArrowUp from '@lucide/svelte/icons/arrow-up';
	import Maximize2 from '@lucide/svelte/icons/maximize-2';
	import MessageSquare from '@lucide/svelte/icons/message-square';
	import Minimize2 from '@lucide/svelte/icons/minimize-2';
	import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Square from '@lucide/svelte/icons/square';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import X from '@lucide/svelte/icons/x';
	import type { OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import type { InboxItem } from '$lib/modules/inbox/types';
	import { areaFor } from '$lib/nav';
	import { useNotifications } from '$lib/platform/shell/notifications.svelte';
	import { useShellCommand } from '$lib/platform/shell/shell-command';
	import { uiPrefs } from '$lib/platform/shell/ui-preferences.svelte';
	import { splitCanonicalRefSegments } from './api';
	import TessActionProposalCard from './TessActionProposalCard.svelte';
	import TessEntityList from './TessEntityList.svelte';
	import TessToolProgress from './TessToolProgress.svelte';
	import { TESS_STARTERS, type TessPanelState } from './tess-panel.svelte';

	interface Props {
		panel: TessPanelState;
		organizationId?: string;
		snapshot?: OrgSnapshot;
		items?: readonly WorkItemSummary[];
		inboxItems?: readonly InboxItem[];
		offices?: readonly OfficeInfo[];
		ownerId?: string;
		offline?: boolean;
		onCloseRestoreFocus?: () => void;
	}

	let {
		panel,
		organizationId,
		snapshot,
		items = [],
		inboxItems = [],
		offices = [],
		ownerId = 'owner',
		offline = false,
		onCloseRestoreFocus
	}: Props = $props();

	const shellCommand = useShellCommand();
	const notifications = useNotifications();
	const routeLabel = $derived(areaFor(page.url.pathname) ?? 'Home');
	const activeTessJobs = $derived(notifications?.activeTessJobs ?? []);
	const isMac = $derived(
		typeof navigator !== 'undefined' && /Mac|iPhone|iPad|iPod/i.test(navigator.platform || '')
	);
	const toggleShortcut = $derived(isMac ? '⌘/' : 'Ctrl /');
	const expandShortcut = $derived(isMac ? '⌘⇧E' : 'Ctrl Shift E');

	const isTessChatActive = $derived(
		page.url.pathname === '/chat' &&
			(page.url.searchParams.get('c')?.toLowerCase().includes('tess') ?? false)
	);

	$effect(() => {
		panel.syncRestingVisibility(uiPrefs.showTessPill);
	});

	const visible = $derived(!isTessChatActive && panel.mode !== 'hidden');
	const isPill = $derived(visible && panel.mode === 'pill');
	const isOpen = $derived(visible && panel.mode === 'open');
	const isExpanded = $derived(visible && panel.mode === 'expanded');
	const isCompact = $derived(isOpen && panel.turns.length === 0);

	$effect(() => {
		if (typeof document === 'undefined') return;
		document.body.classList.toggle('companion-open', isOpen);
		document.body.classList.toggle('companion-minimized', isPill);
		return () => {
			document.body.classList.remove('companion-open', 'companion-minimized');
		};
	});

	const ambientLabel = $derived(
		panel.contextDismissed
			? undefined
			: panel.decisionContext
				? panel.decisionContext.title
				: routeLabel
	);

	const askedSet = $derived(new Set(panel.turns.map((t) => t.query.toLowerCase())));
	const remainingStarters = $derived(
		TESS_STARTERS.filter((s) => !askedSet.has(s.toLowerCase())).slice(
			0,
			panel.turns.length > 0 ? 2 : 3
		)
	);

	const statusLabel = $derived(
		activeTessJobs.length > 0
			? `Refining (${activeTessJobs.length})`
			: panel.isStreaming
				? 'Thinking…'
				: offline
					? 'Offline · Local summary'
					: panel.runtimeStatus === 'ready'
						? 'Chief of Staff · Ready'
						: panel.runtimeStatus === 'no_runtime'
							? 'No runtime · Local summary'
							: 'Offline · Local summary'
	);

	const latestTurn = $derived(panel.turns.at(-1));
	const latestFollowUps = $derived(
		latestTurn && !latestTurn.streaming && latestTurn.followUps?.length
			? latestTurn.followUps.slice(0, 3)
			: []
	);

	let pillBtnEl = $state<HTMLButtonElement | null>(null);

	const focusOnMount: Attachment<HTMLInputElement> = (el) => {
		el.focus({ preventScroll: true });
	};

	function scrollOnTurns(count: number): Attachment<HTMLDivElement> {
		return (el) => {
			if (count > 0) el.scrollTop = el.scrollHeight;
		};
	}

	async function openTessDmInChat() {
		panel.close(uiPrefs.showTessPill);
		await shellCommand?.openTessChat();
	}

	function submitPrompt(raw: string) {
		const trimmed = raw.trim();
		if (!trimmed) return;
		panel.ask(trimmed, {
			organizationId,
			snapshot,
			items,
			offices,
			ownerId,
			domain: { inboxItems },
			routeLabel,
			routePath: page.url.pathname,
			offline
		});
	}

	function handleArtifactNavigate() {
		if (isExpanded) panel.toggleExpand();
	}

	function closeCompanion(restoreFocus = true) {
		panel.close(uiPrefs.showTessPill);
		if (!restoreFocus) return;
		void tick().then(() => {
			if (uiPrefs.showTessPill && pillBtnEl) pillBtnEl.focus({ preventScroll: true });
			else onCloseRestoreFocus?.();
		});
	}

	function handleWindowKeydown(event: KeyboardEvent) {
		if (!isOpen && !isExpanded) return;
		if (event.key !== 'Escape') return;
		if (document.querySelector('[role="alertdialog"], [role="menu"]')) return;
		if (!isExpanded && document.querySelector('[role="dialog"]:not([data-testid="tess-panel"])')) {
			return;
		}
		event.preventDefault();
		if (isExpanded) {
			panel.toggleExpand();
		} else {
			closeCompanion(true);
		}
	}
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if visible}
	{#if isExpanded}
		<button
			type="button"
			aria-label="Restore Tess companion size"
			tabindex={-1}
			onclick={() => panel.toggleExpand()}
			class="tess-panel-scrim"
		></button>
	{/if}

	<aside
		role={isExpanded ? 'dialog' : 'complementary'}
		aria-modal={isExpanded ? 'true' : undefined}
		aria-label="Tess Chief of Staff"
		data-testid="tess-panel"
		class={[
			'tess-panel',
			isPill && 'is-minimized',
			isCompact && 'is-compact',
			isExpanded && 'is-expanded'
		]}
	>
		{#if isPill}
			<button
				bind:this={pillBtnEl}
				type="button"
				data-testid="tess-dock-pill"
				onclick={() => panel.openPanel()}
				aria-label={activeTessJobs.length > 0
					? `Tess refining (${activeTessJobs.length}) · ${toggleShortcut}`
					: `Ask Tess · ${toggleShortcut}`}
				aria-keyshortcuts="Meta+/ Control+/"
				title={activeTessJobs.length > 0
					? `Tess refining (${activeTessJobs.length}) · ${toggleShortcut}`
					: `Ask Tess · ${toggleShortcut}`}
				class="cmp-dock-btn"
			>
				{#if offline}
					<WifiOff class="cmp-dock-ico size-3.5 shrink-0" aria-hidden="true" />
				{:else}
					<Sparkles class="cmp-dock-ico size-3.5 shrink-0" aria-hidden="true" />
				{/if}
				<strong>Tess</strong>
				{#if activeTessJobs.length > 0}
					<span class="cmp-pill-st">Refining ({activeTessJobs.length})</span>
					<span class="tess-live-dot" aria-hidden="true"></span>
				{:else if offline}
					<span class="cmp-pill-st">Offline</span>
				{:else}
					<kbd class="cmp-pill-kbd" aria-hidden="true">{toggleShortcut}</kbd>
				{/if}
			</button>
		{:else}
			<header class="cmp-hd">
				<div class="cmp-tess-who">
					<span class="cmp-tess-mark" aria-hidden="true">
						{#if offline}
							<WifiOff class="size-3.5" />
						{:else}
							<Sparkles class="size-3.5" />
						{/if}
					</span>
					<div class="cmp-tess-txt">
						<strong>Tess</strong>
						<span class={['cmp-tess-st', offline && 'is-offline']}>{statusLabel}</span>
					</div>
				</div>
				<div class="cmp-hd-acts">
					<button
						type="button"
						onclick={() => void openTessDmInChat()}
						aria-label="Open DM with Tess in Chat"
						title="Open DM with Tess in Chat"
						class="cmp-chat-btn"
					>
						<MessageSquare class="size-3.5 shrink-0" aria-hidden="true" />
						<span>Chat</span>
					</button>
					{#if panel.turns.length > 0}
						<button
							type="button"
							onclick={() => void panel.resetSession(organizationId)}
							title="Start a new Tess thread"
							class="cmp-text-btn inline-flex items-center gap-1"
						>
							<RotateCcw class="size-3" aria-hidden="true" />
							<span>New thread</span>
						</button>
					{/if}
					<button
						type="button"
						onclick={() => panel.toggleExpand()}
						aria-label="{isExpanded ? 'Restore size' : 'Maximize'} · {expandShortcut}"
						aria-keyshortcuts="Meta+Shift+E Control+Shift+E"
						title="{isExpanded ? 'Restore size' : 'Maximize'} · {expandShortcut}"
						class="cmp-icon-btn cmp-expand-btn"
					>
						{#if isExpanded}
							<Minimize2 class="size-3.5" aria-hidden="true" />
						{:else}
							<Maximize2 class="size-3.5" aria-hidden="true" />
						{/if}
					</button>
					<button
						type="button"
						onclick={() => closeCompanion(true)}
						aria-label="{uiPrefs.showTessPill ? 'Minimize to bottom' : 'Close'} · Esc"
						title="{uiPrefs.showTessPill ? 'Minimize to bottom' : 'Close'} · Esc"
						class="cmp-icon-btn"
					>
						<X class="size-3.5" aria-hidden="true" />
					</button>
				</div>
			</header>

			{#if ambientLabel}
				<div class="cmp-ctx-row">
					<span class="cmp-ctx-pill">
						<span class="cmp-ctx-lbl">Context: <strong>{ambientLabel}</strong></span>
						<button
							type="button"
							onclick={() => panel.dismissContext()}
							aria-label="Detach page context"
						>
							<X class="size-3" aria-hidden="true" />
						</button>
					</span>
				</div>
			{/if}

			{#if activeTessJobs.length > 0}
				<div class="tess-busy-strip" role="status">
					<span class="tess-busy-spin" aria-hidden="true"></span>
					<span>
						{activeTessJobs.length > 1
							? `Refining ${activeTessJobs.length} tasks in background…`
							: `${activeTessJobs[0].title} — ${activeTessJobs[0].step}`}
					</span>
				</div>
			{/if}

			{#if offline || panel.runtimeStatus !== 'ready'}
				<div class="tess-offline-note" role="status">
					<WifiOff class="size-3.5 shrink-0" aria-hidden="true" />
					<span class="min-w-0 flex-1">
						{panel.runtimeStatus === 'no_runtime'
							? 'No runtime configured · Offline quick summary'
							: 'Offline quick summary · Local data only'}
					</span>
					<a
						href={resolve('/settings/agents')}
						onclick={handleArtifactNavigate}
						class="tess-connect-link"
					>
						Connect runtime →
					</a>
				</div>
			{/if}

			<span class="sr-only" aria-live="polite">{panel.liveAnnouncement}</span>

			<div {@attach scrollOnTurns(panel.turns.length)} class="tess-body">
				{#if panel.turns.length > 0}
					<div class="tess-thread">
						{#each panel.turns as turn (turn.id)}
							<div class="tess-turn">
								<div class="tess-q">{turn.query}</div>
								<article class="tess-a">
									<div class="tess-a-hd">
										<Sparkles class="size-3" aria-hidden="true" />
										<span>Tess</span>
										{#if turn.mode === 'offline_summary'}
											<span class="tess-a-mode">Offline quick summary · Local data only</span>
										{:else if turn.streaming}
											<span class="tess-a-mode">Live</span>
										{/if}
									</div>

									{#if turn.toolCalls?.length || turn.streaming}
										<TessToolProgress
											calls={turn.toolCalls ?? []}
											streaming={Boolean(turn.streaming)}
											onCancel={turn.streaming ? () => void panel.stopActiveTurn() : undefined}
										/>
									{/if}

									{#if turn.errorMessage}
										<p class="tess-turn-err" role="alert">{turn.errorMessage}</p>
									{/if}

									<div class="tess-a-body">
										{#if turn.mode === 'live'}
											{#if turn.streamText}
												<p class="whitespace-pre-wrap text-[var(--exec-body)]">
													{#each splitCanonicalRefSegments(turn.streamText) as seg, idx (`${idx}-${seg.text}`)}
														{#if seg.href}
															<a
																href={resolve(seg.href as '/')}
																onclick={handleArtifactNavigate}
																class="tess-ref-link"
															>
																{seg.text}
															</a>
														{:else}
															{seg.text}
														{/if}
													{/each}
												</p>
											{/if}
										{:else}
											<p class="font-semibold text-[var(--exec-ink,var(--fg))]">
												{turn.answer.headline}
											</p>
											{#if turn.answer.bullets.length > 0}
												<ul class="mt-1.5 flex flex-col gap-1">
													{#each turn.answer.bullets as bullet (bullet.text)}
														<li class="flex flex-wrap items-baseline gap-x-2">
															<span>• {bullet.text}</span>
															{#if bullet.href && bullet.linkLabel}
																<a
																	href={resolve(bullet.href as '/')}
																	onclick={handleArtifactNavigate}
																>
																	{bullet.linkLabel}
																</a>
															{/if}
														</li>
													{/each}
												</ul>
											{/if}
										{/if}

										{#if turn.blocks?.length}
											{#each turn.blocks as block, bIdx (`${turn.id}-blk-${bIdx}`)}
												{#if block.kind === 'action_proposal'}
													<TessActionProposalCard
														proposal={block.proposal}
														onApply={(id) => void panel.applyProposal(turn.id, id)}
														onDismiss={(id) => void panel.dismissProposal(turn.id, id)}
														onNavigate={handleArtifactNavigate}
													/>
												{:else if block.kind === 'entity_list' || block.kind === 'navigation'}
													<TessEntityList
														title={block.title}
														items={block.items}
														onNavigate={handleArtifactNavigate}
													/>
												{:else if block.kind === 'metrics'}
													<TessEntityList
														title={block.title}
														metrics={block.metrics}
														onNavigate={handleArtifactNavigate}
													/>
												{/if}
											{/each}
										{/if}
									</div>
								</article>
							</div>
						{/each}
					</div>
				{/if}

				{#if latestFollowUps.length > 0}
					<div class="tess-chips" aria-label="Follow-up prompts">
						{#each latestFollowUps as follow (follow)}
							<button type="button" class="tess-chip" onclick={() => submitPrompt(follow)}>
								<Sparkles class="cmp-chip-ico size-3.5 shrink-0" aria-hidden="true" />
								<span>{follow}</span>
							</button>
						{/each}
					</div>
				{:else if remainingStarters.length > 0}
					<div class="tess-chips" aria-label="Suggested prompts">
						{#each remainingStarters as starter (starter)}
							<button type="button" class="tess-chip" onclick={() => submitPrompt(starter)}>
								<Sparkles class="cmp-chip-ico size-3.5 shrink-0" aria-hidden="true" />
								<span>{starter}</span>
							</button>
						{/each}
					</div>
				{/if}
			</div>

			<form
				onsubmit={(event) => {
					event.preventDefault();
					submitPrompt(panel.draft);
				}}
				class="tess-compose"
			>
				<input
					{@attach focusOnMount}
					bind:value={panel.draft}
					type="text"
					autocomplete="off"
					aria-label="Ask Tess anything"
					placeholder="Ask Tess anything…"
				/>
				{#if panel.isStreaming}
					<button
						type="button"
						onclick={() => void panel.stopActiveTurn()}
						aria-label="Stop Tess turn"
						class="tess-send-btn is-stop"
					>
						<Square class="size-3.5 fill-current" aria-hidden="true" />
					</button>
				{:else}
					<button
						type="submit"
						disabled={!panel.draft.trim()}
						aria-label="Send to Tess"
						class="tess-send-btn"
					>
						<ArrowUp class="size-4" aria-hidden="true" />
					</button>
				{/if}
			</form>
		{/if}
	</aside>
{/if}

<style>
	.tess-panel-scrim {
		position: fixed;
		inset: 0;
		z-index: 82;
		background: var(--exec-scrim-bg, color-mix(in srgb, var(--surface-sunken) 72%, transparent));
		animation: cmp-scrim-in 0.15s ease-out both;
	}
	.tess-panel {
		position: fixed;
		right: 20px;
		bottom: 20px;
		z-index: 84;
		width: 420px;
		max-width: calc(100vw - 32px);
		max-height: min(560px, calc(100dvh - 108px));
		display: flex;
		flex-direction: column;
		overflow: hidden;
		border: 1px solid var(--exec-tess-border, var(--exec-border-hi, var(--line-strong)));
		border-radius: var(--exec-tess-radius, 16px);
		background: var(--exec-tess-bg, var(--exec-cmdk-menu-bg, var(--surface)));
		-webkit-backdrop-filter: var(--exec-tess-blur, var(--exec-cmdk-menu-blur, none));
		backdrop-filter: var(--exec-tess-blur, var(--exec-cmdk-menu-blur, none));
		color: var(--exec-ink, var(--fg));
		box-shadow: var(--exec-tess-shadow, var(--exec-cmdk-menu-shadow, none));
		transform-origin: bottom right;
		will-change: transform, opacity;
		animation: cmp-open 0.18s cubic-bezier(0.16, 1, 0.3, 1) both;
	}
	.tess-panel.is-compact {
		max-height: min(320px, calc(100dvh - 108px));
	}
	.tess-panel.is-minimized {
		width: auto;
		max-width: min(260px, calc(100vw - 32px));
		height: 42px;
		border-radius: 999px;
		overflow: visible;
		background: var(
			--exec-tess-pill-bg,
			var(--exec-tess-bg, var(--exec-cmdk-menu-bg, var(--surface)))
		);
		border-color: var(--exec-tess-pill-border, var(--exec-tess-border, var(--exec-border-hi)));
		box-shadow: var(--exec-tess-pill-shadow, var(--exec-cmdk-menu-shadow, none));
		animation: cmp-pill 0.16s cubic-bezier(0.16, 1, 0.3, 1) both;
	}
	.tess-panel.is-expanded {
		top: 50%;
		left: 50%;
		right: auto;
		bottom: auto;
		z-index: 86;
		width: min(680px, calc(100vw - 48px));
		height: min(74dvh, 660px);
		max-height: calc(100dvh - 48px);
		transform: translate(-50%, -50%);
		transform-origin: center;
		animation: cmp-expand 0.2s cubic-bezier(0.16, 1, 0.3, 1) both;
	}
	:global(body:has(.activity-tools)) .tess-panel:not(.is-expanded) {
		right: 68px;
	}
	@keyframes cmp-scrim-in {
		from {
			opacity: 0;
		}
		to {
			opacity: 1;
		}
	}
	@keyframes cmp-open {
		from {
			opacity: 0;
			transform: translate3d(0, 14px, 0) scale(0.94);
		}
		to {
			opacity: 1;
			transform: translate3d(0, 0, 0) scale(1);
		}
	}
	@keyframes cmp-pill {
		from {
			opacity: 0.65;
			transform: translate3d(0, -8px, 0) scale(1.06);
		}
		to {
			opacity: 1;
			transform: translate3d(0, 0, 0) scale(1);
		}
	}
	@keyframes cmp-expand {
		from {
			opacity: 0;
			transform: translate3d(-50%, -47%, 0) scale(0.95);
		}
		to {
			opacity: 1;
			transform: translate3d(-50%, -50%, 0) scale(1);
		}
	}

	.cmp-dock-btn {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		height: 40px;
		padding: 0 13px;
		border-radius: 999px;
		color: var(--exec-ink, var(--fg));
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
		transition: background-color 0.14s ease;
	}
	.cmp-dock-btn:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
	}
	.cmp-dock-btn :global(.cmp-dock-ico) {
		color: var(--tone-accent-ink, var(--link));
	}
	.cmp-pill-st {
		font-size: 0.75rem;
		color: var(--exec-muted, var(--fg-muted));
		font-weight: 500;
	}
	.cmp-pill-kbd {
		padding: 1px 6px;
		border-radius: 5px;
		background: var(--exec-well, var(--surface-raised));
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		color: var(--exec-muted, var(--fg-muted));
		font-family: inherit;
		font-size: 0.75rem;
		font-weight: 600;
	}
	.tess-live-dot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--tone-accent, var(--brand));
		box-shadow: 0 0 0 2px var(--tone-accent-bg);
		animation: tess-pulse 1.1s ease-in-out infinite;
	}
	@keyframes tess-pulse {
		0%,
		100% {
			opacity: 0.4;
			transform: scale(0.85);
		}
		50% {
			opacity: 1;
			transform: scale(1.15);
		}
	}

	.cmp-hd {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		padding: 10px 12px;
		border-bottom: 1px solid var(--exec-border, var(--line));
		flex: none;
	}
	.cmp-tess-who {
		display: flex;
		align-items: center;
		gap: 9px;
		min-width: 0;
	}
	.cmp-tess-mark {
		width: 28px;
		height: 28px;
		border-radius: 8px;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		background: var(--tone-accent-bg, var(--exec-accent-soft));
		border: 1px solid var(--tone-accent-ring, var(--exec-accent-ring));
		color: var(--tone-accent-ink, var(--link));
		flex: none;
	}
	.cmp-tess-txt {
		display: grid;
		gap: 1px;
		line-height: 1.2;
		min-width: 0;
	}
	.cmp-tess-txt strong {
		font-size: 0.875rem;
		font-weight: 650;
	}
	.cmp-tess-st {
		font-size: 0.75rem;
		color: var(--exec-muted, var(--fg-muted));
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.cmp-tess-st.is-offline {
		color: var(--tone-bad-ink);
	}
	.cmp-hd-acts {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		flex: none;
	}
	.cmp-chat-btn {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		height: 28px;
		padding: 0 8px;
		border-radius: 6px;
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		background: var(--exec-well, var(--surface-raised));
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.75rem;
		font-weight: 550;
		cursor: pointer;
	}
	.cmp-chat-btn:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink, var(--fg));
	}
	.cmp-text-btn {
		padding: 4px 7px;
		border-radius: 6px;
		color: var(--tone-accent-ink, var(--link));
		font-size: 0.75rem;
		font-weight: 600;
		cursor: pointer;
	}
	.cmp-text-btn:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
		color: var(--exec-ink, var(--fg));
	}
	.cmp-icon-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 28px;
		height: 28px;
		border-radius: 6px;
		color: var(--exec-muted, var(--fg-muted));
		cursor: pointer;
	}
	.cmp-icon-btn:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
		color: var(--exec-ink, var(--fg));
	}

	.cmp-ctx-row {
		padding: 6px 12px;
		border-bottom: 1px solid var(--exec-border, var(--line));
		flex: none;
	}
	.cmp-ctx-pill {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		max-width: 100%;
		padding: 2px 6px 2px 9px;
		border-radius: 999px;
		background: var(--exec-well, var(--surface-raised));
		border: 1px solid var(--exec-border, var(--line));
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.75rem;
	}
	.cmp-ctx-lbl {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.cmp-ctx-lbl strong {
		color: var(--exec-ink, var(--fg));
		font-weight: 600;
	}
	.cmp-ctx-pill button {
		display: inline-flex;
		padding: 2px;
		border-radius: 50%;
		color: var(--exec-muted, var(--fg-muted));
		flex: none;
		cursor: pointer;
	}
	.cmp-ctx-pill button:hover {
		color: var(--exec-ink, var(--fg));
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
	}

	.tess-busy-strip,
	.tess-offline-note {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 7px 12px;
		border-bottom: 1px solid var(--exec-border, var(--line));
		font-size: 0.75rem;
		font-weight: 550;
		flex: none;
	}
	.tess-busy-strip {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
	}
	.tess-busy-spin {
		width: 14px;
		height: 14px;
		border-radius: 50%;
		border: 2px solid var(--tone-accent-ring);
		border-top-color: var(--tone-accent);
		flex: none;
		animation: tess-spin 0.85s linear infinite;
	}
	@keyframes tess-spin {
		to {
			transform: rotate(360deg);
		}
	}
	.tess-offline-note {
		background: var(--tone-warn-bg);
		color: var(--tone-warn-ink);
	}

	.tess-body {
		flex: 1 1 auto;
		overflow-y: auto;
		padding: 12px;
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.tess-thread {
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.tess-turn {
		display: grid;
		gap: 6px;
	}
	.tess-q {
		justify-self: end;
		max-width: 86%;
		padding: 7px 11px;
		border-radius: 12px 12px 4px 12px;
		background: var(--exec-tess-q-bg, var(--tone-accent-bg));
		border: 1px solid var(--exec-tess-q-border, var(--tone-accent-ring));
		color: var(--exec-ink, var(--fg));
		font-size: 0.8125rem;
		line-height: 1.45;
	}
	.tess-a {
		padding: 10px 12px;
		border-radius: 10px;
		background: var(--exec-tess-card-bg, var(--exec-well, var(--surface-raised)));
		border: 1px solid var(--exec-tess-card-border, var(--exec-border, var(--line)));
		display: grid;
		gap: 5px;
		font-size: 0.8125rem;
		line-height: 1.5;
		color: var(--exec-body, var(--fg-secondary));
	}
	.tess-a-hd {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		color: var(--tone-accent-ink, var(--link));
		font-size: 0.75rem;
		font-weight: 650;
	}
	.tess-a-mode {
		margin-inline-start: auto;
		font-size: 0.75rem;
		font-weight: 500;
		color: var(--exec-muted, var(--fg-muted));
	}
	.tess-a-body :global(a) {
		color: var(--tone-accent-ink, var(--link));
		text-decoration: underline;
		text-underline-offset: 2px;
		font-size: 0.75rem;
		font-weight: 600;
	}

	.tess-chips {
		display: flex;
		flex-direction: column;
		gap: 5px;
		margin-top: auto;
	}
	.tess-chip {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 7px 10px;
		border-radius: 9px;
		border: 1px solid var(--exec-tess-card-border, var(--exec-border-hi, var(--line-strong)));
		background: var(--exec-tess-card-bg, var(--exec-well, var(--surface-raised)));
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.8125rem;
		text-align: left;
		cursor: pointer;
		transition:
			border-color 0.14s ease,
			color 0.14s ease,
			background-color 0.14s ease;
	}
	.tess-chip:hover {
		border-color: var(--tone-accent-ring);
		background: var(--tone-accent-bg);
		color: var(--exec-ink, var(--fg));
	}
	.tess-chip :global(.cmp-chip-ico) {
		color: var(--tone-accent-ink, var(--link));
	}

	.tess-compose {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 9px 10px;
		border-top: 1px solid var(--exec-border, var(--line));
		flex: none;
	}
	.tess-compose input {
		flex: 1 1 auto;
		min-width: 0;
		height: 36px;
		padding: 0 11px;
		border: 1px solid var(--exec-tess-card-border, var(--exec-border-hi, var(--line-strong)));
		border-radius: 10px;
		background: var(--exec-tess-card-bg, var(--exec-well, var(--surface-raised)));
		color: var(--exec-ink, var(--fg));
		font: inherit;
		font-size: 0.875rem;
		outline: none;
	}
	.tess-compose input::placeholder {
		color: var(--exec-muted, var(--fg-muted));
	}
	.tess-compose input:focus {
		border-color: var(--tone-accent, var(--focus));
	}
	.tess-send-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 36px;
		height: 36px;
		border-radius: 10px;
		flex: none;
		background: var(--exec-cta-bg, var(--primary));
		color: var(--exec-on-accent, var(--on-emphasis));
		cursor: pointer;
	}
	.tess-send-btn:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}
	.tess-send-btn.is-stop {
		background: var(--tone-bad);
		color: var(--exec-on-accent);
	}
	.tess-connect-link {
		flex: none;
		font-weight: 600;
		color: var(--exec-accent-ink);
		text-decoration: none;
		white-space: nowrap;
	}
	.tess-connect-link:hover {
		text-decoration: underline;
	}
	.tess-turn-err {
		margin-bottom: 6px;
		padding: 7px 10px;
		border-radius: var(--exec-sm-radius, 8px);
		background: var(--tone-bad-bg);
		color: var(--tone-bad-ink);
		box-shadow: inset 0 0 0 1px var(--tone-bad-ring);
		font-size: 0.78rem;
		line-height: 1.4;
	}
	.tess-ref-link {
		font-family: var(--font-mono);
		font-size: 0.8125rem;
		font-weight: 600;
		color: var(--exec-accent-ink);
		text-decoration: underline;
		text-underline-offset: 2px;
	}

	@media (max-width: 639px) {
		.tess-panel:not(.is-minimized) {
			inset: auto 0 0 0;
			width: 100vw;
			max-width: 100vw;
			height: auto;
			max-height: 82dvh;
			border-radius: 16px 16px 0 0;
			border-bottom: 0;
			transform: none;
		}
		.cmp-expand-btn {
			display: none;
		}
		.tess-compose input {
			font-size: 1rem;
			height: 44px;
		}
		.tess-send-btn {
			width: 44px;
			height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.tess-panel,
		.tess-panel-scrim,
		.tess-live-dot,
		.tess-busy-spin {
			animation: none;
		}
	}
</style>
