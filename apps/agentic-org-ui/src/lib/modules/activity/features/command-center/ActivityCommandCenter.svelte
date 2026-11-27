<script lang="ts">
	import { onDestroy, type Snippet } from 'svelte';
	import { Tooltip } from 'bits-ui';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import { activityApi } from '$lib/modules/activity/api';
	import type { ActivityQuery } from '$lib/modules/activity/types';
	import { inboxApi } from '$lib/modules/inbox/api';
	import type { ActivityEntry } from '$lib/modules/members';
	import { accountZone } from '$lib/modules/preferences';
	import type { ReleaseMilestone } from '$lib/modules/releases/types';
	import { useShellData } from '$lib/platform/shell/shell-data.svelte';
	import type { OrgSnapshot } from '$lib/proto';
	import ActivityDockControls from './ActivityDockControls.svelte';
	import ActivityLiveBanner from './ActivityLiveBanner.svelte';
	import ActivityOpsRail from './ActivityOpsRail.svelte';
	import ActivityPortfolioHeader from './ActivityPortfolioHeader.svelte';
	import ActivityProjectToolbar from './ActivityProjectToolbar.svelte';
	import ActivityStreamPanel from './ActivityStreamPanel.svelte';
	import AssemblyLineMatrix from './AssemblyLineMatrix.svelte';
	import { ActivityFeedController } from './activity-feed-controller.svelte';
	import {
		attachPanelEscapeListener,
		buildExtraLineEvents,
		extractWorkErrorMessage,
		mergeProjectStreamEntries,
		moveProjectViewTabFocus,
		observeScrollOverflow,
		scrollPanelByPage
	} from './activity-project-merge';
	import {
		deriveActivityCommandCenter,
		deriveDecisionStreamEntries,
		type FeedPhase,
		type StreamFilterTab
	} from './assembly-model';

	let {
		organizationId,
		organizationName,
		snapshot,
		lockedProjectId = undefined,
		scopedTeamIds = undefined,
		releaseMilestones = [],
		extraProjectActivity = [],
		onRegisterToolbar = undefined
	}: {
		organizationId: string;
		organizationName: string;
		snapshot: OrgSnapshot;
		lockedProjectId?: string;
		scopedTeamIds?: readonly string[];
		releaseMilestones?: readonly ReleaseMilestone[];
		extraProjectActivity?: readonly ActivityEntry[];
		onRegisterToolbar?: (controls: Snippet | undefined) => void;
	} = $props();

	const shellData = useShellData();
	const isProjectMode = $derived(Boolean(lockedProjectId));

	// CRITICAL DEFAULT: Assembly Line Matrix is shown FIRST by default.
	let viewMode = $state<'assembly' | 'feed'>('assembly');
	let panelOpen = $state(typeof window !== 'undefined' ? window.innerWidth >= 1024 : true);
	let toolsBtnEl = $state<HTMLButtonElement | null>(null);
	let panelScrollEl = $state<HTMLDivElement | null>(null);
	let hasMorePanelScroll = $state(false);
	let displayMenuOpen = $state(false);
	let filterMenuOpen = $state(false);
	let projectScope = $state<string>('all');
	let streamFilter = $state<StreamFilterTab>('all');
	let focusedProjectView = $state<'assembly' | 'feed' | null>(null);
	let destroyed = false;

	const shellReady = $derived(
		shellData.loaded && shellData.loadedOrganizationId === organizationId
	);
	const workUnavailableReason = $derived(
		shellReady && shellData.workError ? extractWorkErrorMessage(shellData.workError) : undefined
	);
	const workPhase = $derived<FeedPhase>(
		!shellReady ? 'loading' : workUnavailableReason ? 'error' : 'ready'
	);
	const liveSnapshot = $derived(shellReady && shellData.snapshot ? shellData.snapshot : snapshot);
	const workItems = $derived(workPhase === 'ready' ? shellData.workItems : []);
	const activeProjectIds = $derived(
		new Set((liveSnapshot.projects ?? []).filter((p) => !p.archived).map((p) => p.id))
	);
	const effectiveProjectScope = $derived(
		lockedProjectId
			? lockedProjectId
			: projectScope === 'all' || activeProjectIds.has(projectScope)
				? projectScope
				: 'all'
	);
	const activeAllQuery = $derived<ActivityQuery>(
		effectiveProjectScope !== 'all'
			? { kind: 'all', project: effectiveProjectScope }
			: { kind: 'all' }
	);
	const activeDecisionQuery = $derived<ActivityQuery>(
		effectiveProjectScope !== 'all'
			? { kind: 'decision', project: effectiveProjectScope }
			: { kind: 'decision' }
	);

	function restoreFocusIfBannerDismissing(id: string) {
		if (destroyed || typeof document === 'undefined' || id !== organizationId) return;
		if (feed.isBannerStillVisible(id)) return;
		const active = document.activeElement;
		if (active instanceof HTMLElement && active.closest('.cmd-live-banner')) {
			const fallbackTarget =
				active
					.closest('.cmd-page')
					?.querySelector<HTMLElement>(
						'.cmd-status-slot, .assembly-view, .stream-body, .activity-panel-scroll'
					) ?? document.getElementById('project-view-tab-assembly');
			fallbackTarget?.focus();
		}
	}

	const feed = new ActivityFeedController({
		getOrganizationId: () => organizationId,
		getActiveAllQuery: () => activeAllQuery,
		getActiveDecisionQuery: () => activeDecisionQuery,
		isDecisionTabActive: () => viewMode === 'feed' && streamFilter === 'decision',
		refreshShell: () => shellData.refresh(),
		onBannerMayDismiss: restoreFocusIfBannerDismissing
	});

	const inboxItems = $derived(feed.inboxPhase === 'ready' ? (feed.directInboxItems ?? []) : []);
	const inboxSubFailed = $derived(feed.inboxSubFailedOrg === organizationId);
	const activitySubFailed = $derived(feed.activitySubFailedOrg === organizationId);
	const catchupFailed = $derived(
		feed.inboxCatchupFailedOrg === organizationId ||
			feed.activityCatchupFailedOrg === organizationId ||
			(viewMode === 'feed' &&
				streamFilter === 'decision' &&
				feed.decisionCatchupFailedOrg === organizationId)
	);
	const isReconnecting = $derived(feed.reconnectingOrg === organizationId);
	const showLiveBanner = $derived(
		inboxSubFailed || activitySubFailed || catchupFailed || isReconnecting
	);

	onDestroy(() => {
		destroyed = true;
		feed.dispose();
		onRegisterToolbar?.(undefined);
	});

	$effect(() => {
		if (!isProjectMode || !onRegisterToolbar) return;
		onRegisterToolbar(projectToolbarControls);
		return () => onRegisterToolbar(undefined);
	});

	$effect(() => {
		if (!panelOpen || typeof window === 'undefined') return;
		const timer = window.setInterval(() => {
			feed.nowMs = Date.now();
		}, 30_000);
		return () => window.clearInterval(timer);
	});

	function updatePanelScrollMore() {
		if (!panelScrollEl || !panelOpen) {
			hasMorePanelScroll = false;
			return;
		}
		const next =
			panelScrollEl.scrollHeight - panelScrollEl.scrollTop - panelScrollEl.clientHeight > 12;
		if (!next && hasMorePanelScroll && typeof document !== 'undefined') {
			const active = document.activeElement;
			if (active instanceof HTMLElement && active.classList.contains('activity-panel-more')) {
				panelScrollEl.focus({ preventScroll: true });
			}
		}
		hasMorePanelScroll = next;
	}

	$effect(() => {
		const el = panelScrollEl;
		void panelOpen;
		void viewMode;
		void releaseMilestones.length;
		void model.milestones.length;
		void model.lineEvents.length;
		void extraProjectActivity.length;
		if (!el) {
			hasMorePanelScroll = false;
			return;
		}
		return observeScrollOverflow(el, updatePanelScrollMore);
	});

	$effect(() => {
		const id = organizationId;
		void accountZone.ensure(id);
		feed.fetchInbox(id, true);
		return () => feed.resetInboxScope();
	});

	$effect(() => {
		const id = organizationId;
		void feed.inboxSubRetrySeq;
		let cancelled = false;
		let unsub: (() => void) | undefined;
		void inboxApi
			.subscribe(id, () => {
				if (!cancelled) feed.scheduleInboxReconcile(id);
			})
			.then((stop) => {
				if (cancelled) stop();
				else {
					unsub = stop;
					feed.onInboxSubscribed(id);
				}
			})
			.catch(() => {
				if (!cancelled) feed.onInboxSubscribeFailed(id);
			});
		return () => {
			cancelled = true;
			unsub?.();
		};
	});

	$effect(() => {
		const id = organizationId;
		const query = activeAllQuery;
		void feed.feedRetrySeq;
		feed.fetchMainFeed(id, query, true);
		return () => feed.resetMainScope();
	});

	$effect(() => {
		const id = organizationId;
		const query = activeDecisionQuery;
		const active = viewMode === 'feed' && streamFilter === 'decision';
		void feed.decisionRetrySeq;
		if (!active) return;
		feed.fetchDecisionFeed(id, query, true);
		return () => feed.resetDecisionScope(id);
	});

	$effect(() => {
		const id = organizationId;
		void feed.activitySubRetrySeq;
		let cancelled = false;
		let unsub: (() => void) | undefined;
		void activityApi
			.subscribe(id, (row) => {
				if (!cancelled) feed.handleLiveRow(row);
			})
			.then((stop) => {
				if (cancelled) stop();
				else {
					unsub = stop;
					feed.onActivitySubscribed(id);
				}
			})
			.catch(() => {
				if (!cancelled) feed.onActivitySubscribeFailed(id);
			});
		return () => {
			cancelled = true;
			unsub?.();
		};
	});

	const model = $derived(
		deriveActivityCommandCenter({
			snapshot: liveSnapshot,
			organizationName,
			workItems,
			inboxItems,
			activityRows: feed.activityRows ?? [],
			projectScope: effectiveProjectScope,
			scopedTeamIds,
			ownerId: shellData.ownerId,
			feedPhase: feed.feedPhase,
			feedMore: feed.feedMore,
			workPhase,
			inboxPhase: feed.inboxPhase,
			nowMs: feed.nowMs
		})
	);

	const decisionEntries = $derived(
		deriveDecisionStreamEntries({
			snapshot: liveSnapshot,
			decisionRows: feed.decisionRows ?? [],
			workItems,
			inboxItems,
			projectScope: effectiveProjectScope,
			ownerId: shellData.ownerId,
			decisionPhase: feed.decisionPhase,
			workPhase,
			inboxPhase: feed.inboxPhase
		})
	);

	const extraLineEvents = $derived(
		buildExtraLineEvents(
			extraProjectActivity,
			workPhase,
			feed.inboxPhase,
			model.actionableWorkRefs,
			model.openInboxWorkRefs
		)
	);

	const mergedStreamEntries = $derived(
		mergeProjectStreamEntries({
			baseEntries: model.streamEntries,
			extraProjectActivity,
			projectLabel: model.selectedProject?.name ?? organizationName,
			lockedProjectId,
			workPhase,
			inboxPhase: feed.inboxPhase,
			actionableWorkRefs: model.actionableWorkRefs,
			openInboxWorkRefs: model.openInboxWorkRefs
		})
	);

	const headerSubtitle = $derived(
		!shellReady
			? `Loading pipeline and live stream for ${organizationName}…`
			: workUnavailableReason
				? `Work pipeline unavailable in ${organizationName}`
				: model.scopeSubtitle
	);
	const isHeaderRetrying = $derived(
		feed.headerRetryActive && (feed.shellRefreshing || !shellReady || feed.inboxPhase === 'loading')
	);

	function handleReconnect(event: MouseEvent) {
		if (isReconnecting) return;
		(event.currentTarget as HTMLElement | null)?.closest<HTMLElement>('.cmd-live-banner')?.focus();
		feed.reconnectLiveUpdates();
	}

	function handleHeaderRetry(event: MouseEvent) {
		if (isHeaderRetrying) return;
		feed.headerRetryActive = true;
		(event.currentTarget as HTMLElement | null)?.closest<HTMLElement>('.cmd-status-slot')?.focus();
		feed.retryShellAndInbox();
	}

	function handleProjectViewKeydown(event: KeyboardEvent) {
		const nextMode = moveProjectViewTabFocus(event);
		if (nextMode) focusedProjectView = nextMode;
	}

	function attachReleasePanelEscape() {
		return attachPanelEscapeListener(
			() => panelOpen,
			() => {
				panelOpen = false;
				toolsBtnEl?.focus();
			}
		);
	}
</script>

{#snippet projectToolbarControls()}
	<ActivityProjectToolbar
		bind:viewMode
		bind:streamFilter
		bind:displayMenuOpen
		bind:filterMenuOpen
		bind:focusedProjectView
		onViewKeydown={handleProjectViewKeydown}
	/>
{/snippet}

<div
	class="act-page cmd-page activity-workspace"
	data-activity-scope={isProjectMode ? 'project' : 'portfolio'}
	{@attach attachReleasePanelEscape}
>
	{#if showLiveBanner}
		<ActivityLiveBanner
			{isReconnecting}
			subFailed={inboxSubFailed || activitySubFailed}
			onReconnect={handleReconnect}
		/>
	{/if}

	{#if !isProjectMode}
		<ActivityPortfolioHeader
			{headerSubtitle}
			{shellReady}
			{workUnavailableReason}
			inboxPhase={feed.inboxPhase}
			{isHeaderRetrying}
			needsYouCount={model.needsYouCount}
			streamNeedsYouCount={model.streamNeedsYouCount}
			activeProjects={model.activeProjects}
			{effectiveProjectScope}
			bind:viewMode
			bind:streamFilter
			bind:displayMenuOpen
			onProjectScopeChange={(scope) => (projectScope = scope)}
			onHeaderRetry={handleHeaderRetry}
			onViewKeydown={handleProjectViewKeydown}
		/>
	{/if}

	<Tooltip.Provider delayDuration={150} ignoreNonKeyboardFocus>
		<div class="activity-body" data-panel-open={panelOpen ? '' : undefined}>
			<div
				class="activity-main"
				data-view={viewMode}
				id={isProjectMode ? `project-view-panel-${viewMode}` : undefined}
				role={isProjectMode ? 'tabpanel' : undefined}
				aria-labelledby={isProjectMode ? `project-view-tab-${viewMode}` : undefined}
			>
				{#if isProjectMode}
					<div
						id={viewMode === 'assembly' ? 'project-view-panel-feed' : 'project-view-panel-assembly'}
						role="tabpanel"
						aria-labelledby={viewMode === 'assembly'
							? 'project-view-tab-feed'
							: 'project-view-tab-assembly'}
						hidden
					></div>
				{/if}
				{#if viewMode === 'assembly'}
					<AssemblyLineMatrix
						{model}
						loading={!shellReady}
						unavailableReason={workUnavailableReason}
						inboxPhase={feed.inboxPhase}
						projectScope={isProjectMode}
						hasActivityEntries={extraLineEvents.length > 0 || model.lineEvents.length > 0}
						onRetry={() => feed.retryShellAndInbox()}
					/>
				{:else}
					<div class="activity-stream-pad">
						<ActivityStreamPanel
							entries={mergedStreamEntries}
							{decisionEntries}
							needsYouCount={model.streamNeedsYouCount}
							openInboxCount={feed.inboxPhase === 'ready' ? model.openInboxCount : 0}
							actionableWorkCount={workPhase === 'ready' ? model.actionableWorkCount : 0}
							bottleneck={model.bottleneck}
							bind:activeFilter={streamFilter}
							showTabs={false}
							projectScope={isProjectMode}
							nowMs={feed.nowMs}
							feedPhase={feed.feedPhase}
							feedError={feed.feedError}
							feedMore={feed.feedMore}
							decisionPhase={feed.decisionPhase}
							decisionError={feed.decisionError}
							decisionMore={feed.decisionMore}
							inboxPhase={feed.inboxPhase}
							{workPhase}
							onRetry={() => feed.retryFeed()}
							onRetryDecisions={() => feed.retryDecisions()}
						/>
					</div>
				{/if}
			</div>
			<aside
				id="activity-release-panel"
				class="activity-panel"
				aria-label="Release gates and operational context"
				inert={!panelOpen ? true : undefined}
			>
				<!-- scrollable region requires keyboard focus for arrow/page scroll -->
				<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
				<div
					bind:this={panelScrollEl}
					class="activity-panel-scroll"
					role="region"
					aria-label="Release gates sidebar contents"
					tabindex="0"
				>
					<ActivityDockControls
						{panelOpen}
						mode="header"
						onClose={() => {
							panelOpen = false;
							toolsBtnEl?.focus();
						}}
					/>
					<ActivityOpsRail
						{model}
						loading={!shellReady}
						unavailableReason={workUnavailableReason}
						embedded
						projectScope={isProjectMode}
						inboxPhase={feed.inboxPhase}
						zone={accountZone.zone}
						nowMs={feed.nowMs}
						{extraLineEvents}
						{releaseMilestones}
					/>
				</div>

				<button
					type="button"
					class="activity-panel-more"
					data-testid="activity-panel-more"
					hidden={!panelOpen || !hasMorePanelScroll}
					onclick={() => scrollPanelByPage(panelScrollEl)}
				>
					<span>Scroll for more</span>
					<ChevronDown class="size-3.5" aria-hidden="true" />
				</button>
			</aside>

			<ActivityDockControls
				{panelOpen}
				bind:toolsBtnEl
				mode="rail"
				onToggle={() => (panelOpen = !panelOpen)}
			/>
		</div>
	</Tooltip.Provider>
</div>

<style>
	.cmd-page {
		--activity-gutter: 24px;
		--control-height: 36px;
		display: flex;
		flex: 1 1 auto;
		flex-direction: column;
		width: 100%;
		min-width: 0;
		min-height: 0;
		max-width: 100%;
		padding: 0;
		container: activity / inline-size;
	}
	@media (min-width: 640px) {
		.cmd-page {
			--activity-gutter: 32px;
		}
	}
	@media (max-width: 720px), (pointer: coarse) {
		.cmd-page {
			--activity-gutter: 16px;
			--control-height: 44px;
		}
	}
	.activity-body {
		display: grid;
		flex: 1 1 auto;
		min-height: 0;
		min-width: 0;
		max-width: 100%;
		grid-template-columns: minmax(0, 1fr) 0px calc(var(--control-height, 36px) + 16px);
		grid-template-rows: minmax(0, 1fr);
		grid-template-areas: 'main panel tools';
		--activity-panel-padding: clamp(14px, 2cqi, 22px);
	}
	.activity-body[data-panel-open] {
		grid-template-columns: minmax(0, 1fr) clamp(17rem, 27%, 24rem) calc(
				var(--control-height, 36px) + 16px
			);
	}
	.activity-main,
	.activity-panel-scroll {
		min-width: 0;
		overflow: auto;
		overscroll-behavior: contain;
	}
	.activity-main {
		grid-area: main;
		max-width: 100%;
	}
	.activity-main[data-view='assembly'] {
		display: flex;
		flex-direction: column;
	}
	.activity-stream-pad {
		padding: 18px var(--activity-gutter) 32px;
	}
	.activity-panel {
		grid-area: panel;
		display: grid;
		grid-template-rows: minmax(0, 1fr);
		position: relative;
		min-width: 0;
		visibility: hidden;
		z-index: 1;
		background: var(--side-panel-surface, var(--exec-surface));
		border-inline-start: 1px solid var(--line);
		box-shadow: var(--activity-panel-shadow);
	}
	.activity-body[data-panel-open] .activity-panel {
		visibility: visible;
	}
	.activity-panel-scroll {
		overflow-x: hidden;
		scrollbar-width: thin;
		scrollbar-color: var(--fg-secondary) transparent;
		scroll-padding-block-end: var(--control-height, 36px);
	}
	.activity-panel-scroll:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	.activity-panel-more:not([hidden]) {
		position: absolute;
		z-index: 2;
		inset: auto 0 0;
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 6px;
		min-height: var(--control-height, 36px);
		border: 0;
		border-block-start: 1px solid var(--line-strong);
		background: var(--side-panel-surface, var(--exec-surface));
		color: var(--fg-secondary);
		font-size: 0.8125rem;
		font-weight: 550;
		cursor: pointer;
	}
	.activity-panel-more:hover {
		color: var(--exec-ink);
	}
	.activity-panel-more:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	:global(.activity-tooltip-content) {
		z-index: 60;
		padding: 4px 8px;
		border: 1px solid var(--line-strong);
		border-radius: var(--exec-sm-radius, 6px);
		background: var(--side-panel-surface, var(--surface-overlay, var(--exec-surface)));
		color: var(--exec-ink);
		font-size: 0.75rem;
		font-weight: 500;
		line-height: 1.3;
		box-shadow: var(--tooltip-shadow);
		pointer-events: none;
	}
	@container activity (max-width: 42.5rem) {
		.activity-body,
		.activity-body[data-panel-open] {
			grid-template-columns: minmax(0, 1fr) calc(var(--control-height, 36px) + 16px);
			grid-template-rows: minmax(0, 1fr) 0px;
			grid-template-areas:
				'main tools'
				'panel tools';
		}
		.activity-body[data-panel-open] {
			grid-template-rows: minmax(0, 3fr) minmax(0, 2fr);
		}
		.activity-panel {
			border-inline-start: 0;
			border-block-start: 1px solid var(--line);
			box-shadow: none;
		}
	}
	@media (prefers-reduced-motion: no-preference) {
		.activity-body {
			transition: grid-template-columns 220ms cubic-bezier(0.2, 0.7, 0.25, 1);
		}
		.activity-panel {
			transition: visibility 0s 220ms;
		}
		.activity-body[data-panel-open] .activity-panel {
			transition-delay: 0s;
		}
	}
</style>
