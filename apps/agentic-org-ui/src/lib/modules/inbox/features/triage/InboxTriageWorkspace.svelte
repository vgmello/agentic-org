<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { MediaQuery } from 'svelte/reactivity';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { navigating, page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import { accountZone } from '$lib/modules/preferences';
	import { toAppError } from '$lib/proto';
	import { useNotifications } from '$lib/platform/shell/notifications.svelte';
	import { singleKeyShortcutsOn } from '$lib/platform/shell/shortcuts';
	import type { ShellRefreshResult } from '$lib/platform/shell/shell-data.svelte';
	import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import { inboxApi } from '../../api';
	import { readInboxDividersPreference, writeInboxDividersPreference } from '../../device-comments';
	import type { InboxItem, InboxScope } from '../../types';
	import InboxDetailPane from './InboxDetailPane.svelte';
	import InboxNotificationsSplit from './InboxNotificationsSplit.svelte';
	import InboxQueueList from './InboxQueueList.svelte';
	import InboxSnoozeDialog from './InboxSnoozeDialog.svelte';
	import InboxTriageHeader from './InboxTriageHeader.svelte';
	import InboxTriageSkeleton from './InboxTriageSkeleton.svelte';
	import { formatSnoozeError } from './snooze-presets';
	import {
		buildNextTriageUrl,
		createTriageActionHandlers,
		focusElementFallback,
		isTriageSourceFresh,
		selectNextVisibleAfterResolve,
		type TriageActionOutcome,
		type TriageActionResult,
		type TriageUrlPatch
	} from './triage-action-exec';
	import { TriageDraftStore } from './triage-drafts.svelte';
	import {
		deriveTriageItems,
		filterTriageItems,
		flattenGroups,
		groupTriageItems,
		matchesScope,
		neighborCandidatesAfterResolve,
		openWaitingCount,
		parseTriageTab,
		splitHref,
		triageTabCounts,
		type TriageItem,
		type TriageOrder,
		type TriageTab
	} from './triage-model';
	import {
		attachWorkspaceShortcuts,
		computeRelativeSelectionId,
		mergeScopeOptions
	} from './triage-workspace-helpers';

	let {
		organizationId,
		snapshot,
		ownerId,
		workItems,
		inboxItems,
		loading,
		onRefresh
	}: {
		organizationId: string;
		snapshot: OrgSnapshot;
		ownerId: string;
		workItems: readonly WorkItemSummary[];
		inboxItems: readonly InboxItem[];
		loading: boolean;
		onRefresh: () => Promise<ShellRefreshResult>;
	} = $props();

	const wide = new MediaQuery('min-width: 960px');
	const notifications = useNotifications();
	const drafts = new TriageDraftStore();
	const notifTotalCount = $derived(
		(notifications?.runningJobs.length ?? 0) + (notifications?.items.length ?? 0)
	);
	const notifAttentionCount = $derived(
		(notifications?.runningJobs.length ?? 0) + (notifications?.unreadCount ?? 0)
	);

	let viewingNotifications = $derived(
		(page.url.searchParams.get('tab') ?? '').trim().toLowerCase() === 'notifications'
	);
	let activeTab = $derived<TriageTab>(parseTriageTab(page.url.searchParams.get('tab')));
	let selectedId = $derived<string | undefined>(page.url.searchParams.get('item') ?? undefined);
	let mobileDetailOpen = $derived<boolean>(
		!wide.current && Boolean(page.url.searchParams.get('item'))
	);
	let scope = $derived<InboxScope>({
		team: page.url.searchParams.get('team') || undefined,
		project: page.url.searchParams.get('project') || undefined
	});
	let searchQuery = $state('');
	let searchOpen = $state(false);
	let urgentOnly = $state(false);
	let sortOrder = $state<TriageOrder>('priority');
	let showDividers = $state(readInboxDividersPreference());

	let actionSeq = 0;
	let activeAction = $state<{ token: number; orgId: string; itemId: string } | undefined>(
		undefined
	);
	const actionLocked = $derived(Boolean(activeAction && activeAction.orgId === organizationId));
	const busyId = $derived(
		activeAction && activeAction.orgId === organizationId ? activeAction.itemId : undefined
	);
	const shortcutsEnabled = $derived(singleKeyShortcutsOn());
	let selectionVersion = 0;

	let decliningId = $derived.by<string | undefined>(() => {
		void organizationId;
		return undefined;
	});
	let nowMs = $state(Date.now());
	let searchInputEl = $state<HTMLInputElement | null>(null);
	let snoozeDialog = $state<ReturnType<typeof InboxSnoozeDialog>>();
	let snoozeOpener: HTMLElement | null = null;

	$effect(() => {
		void accountZone.ensure(organizationId);
	});

	$effect(() => {
		void inboxItems;
		void workItems;
		nowMs = Date.now();
	});

	$effect(() => {
		const timer = setInterval(() => {
			nowMs = Date.now();
		}, 30_000);
		return () => clearInterval(timer);
	});

	let pendingUrl: URL | undefined;
	let pendingBaseHref = '';
	let navSeq = 0;
	const inboxPathname = resolve('/inbox');
	let destroyed = false;
	onDestroy(() => {
		destroyed = true;
	});

	const isLeavingInbox = () =>
		page.url.pathname !== inboxPathname ||
		Boolean(navigating.to && navigating.to.url.pathname !== inboxPathname);

	const isCurrentAction = (token: number, targetOrg: string) =>
		!destroyed &&
		!isLeavingInbox() &&
		activeAction?.token === token &&
		organizationId === targetOrg;

	const allItems = $derived(deriveTriageItems(workItems, snapshot, ownerId, inboxItems, nowMs));
	const snoozedLoadError = $derived.by(() => {
		void inboxItems;
		return inboxApi.snoozedError?.(organizationId);
	});
	const totalOpenCount = $derived(openWaitingCount(allItems));
	const scopeOpts = $derived(mergeScopeOptions(allItems, snapshot));
	const counts = $derived(triageTabCounts(allItems, scope));
	const filtered = $derived(
		filterTriageItems(allItems, {
			tab: activeTab,
			urgentOnly,
			scope,
			search: searchQuery,
			order: sortOrder
		})
	);
	const groups = $derived(groupTriageItems(filtered, activeTab, nowMs, sortOrder));
	const visibleItems = $derived(flattenGroups(groups));

	const effectiveSelectedId = $derived.by(() => {
		if (selectedId && visibleItems.some((i) => i.id === selectedId)) return selectedId;
		return wide.current ? visibleItems[0]?.id : undefined;
	});
	const selectedItem = $derived(
		effectiveSelectedId ? visibleItems.find((i) => i.id === effectiveSelectedId) : undefined
	);
	const showingMobileDetail = $derived(!wide.current && mobileDetailOpen && Boolean(selectedItem));
	const denyGuardVisible = $derived(Boolean(selectedItem && decliningId === selectedItem.id));
	const isFiltered = $derived(Boolean(urgentOnly || scope.team || scope.project));
	const displayActive = $derived(sortOrder === 'date' || showDividers);
	const hasActiveFilters = $derived(Boolean(searchQuery.trim() || isFiltered));

	let lastFocusedMobileId: string | undefined;
	$effect(() => {
		const id = showingMobileDetail ? selectedItem?.id : undefined;
		if (!id) {
			lastFocusedMobileId = undefined;
			return;
		}
		if (id === lastFocusedMobileId) return;
		let cancelled = false;
		void tick().then(() => {
			if (cancelled || destroyed) return;
			const heading = document.getElementById('triage-detail-heading');
			if (!heading) return;
			heading.focus();
			lastFocusedMobileId = id;
		});
		return () => {
			cancelled = true;
		};
	});

	const activeDeclineReason = $derived(drafts.getDeny(organizationId, selectedItem?.id));
	const activeReplyText = $derived(drafts.getReply(organizationId, selectedItem?.id));
	const activeCommentDraft = $derived(drafts.getComment(organizationId, selectedItem?.id));

	function syncUrl(patch: TriageUrlPatch) {
		if (destroyed || isLeavingInbox()) return;
		const liveHref = page.url.href;
		const hasPending = Boolean(pendingUrl && liveHref === pendingBaseHref);
		const base = hasPending && pendingUrl ? pendingUrl : page.url;
		pendingBaseHref = liveHref;
		const url = buildNextTriageUrl(base, viewingNotifications ? 'notifications' : activeTab, patch);
		const shouldNavigate = hasPending ? pendingUrl?.href !== url.href : url.href !== liveHref;
		if (!shouldNavigate) return;

		const seq = ++navSeq;
		pendingUrl = new URL(url);
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- cloned page.url preserves deployment base path and arbitrary query/hash fragments
		void goto(url, { replaceState: true, keepFocus: true, noScroll: true }).finally(() => {
			if (seq === navSeq) pendingUrl = undefined;
		});
	}

	function selectTab(tab: TriageTab) {
		selectionVersion += 1;
		viewingNotifications = false;
		activeTab = tab;
		decliningId = undefined;
		mobileDetailOpen = false;
		const nextList = filterTriageItems(allItems, {
			tab,
			urgentOnly,
			scope,
			search: searchQuery,
			order: sortOrder
		});
		selectedId = wide.current ? nextList[0]?.id : undefined;
		syncUrl({ tab, item: null });
	}

	function selectNotificationsTab() {
		selectionVersion += 1;
		viewingNotifications = true;
		decliningId = undefined;
		mobileDetailOpen = false;
		syncUrl({ tab: 'notifications', item: null });
	}

	function focusSearchInput() {
		searchOpen = true;
		void tick().then(() => {
			if (!destroyed) searchInputEl?.focus();
		});
	}

	function toggleInlineSearch() {
		if (searchOpen && !searchQuery.trim()) {
			searchOpen = false;
			return;
		}
		focusSearchInput();
	}

	function closeInlineSearch() {
		searchOpen = false;
		document.getElementById('shell-page-title')?.focus({ preventScroll: true });
	}

	function selectItem(id: string) {
		selectionVersion += 1;
		selectedId = id;
		decliningId = undefined;
		if (!wide.current) mobileDetailOpen = true;
		syncUrl({ item: id });
	}

	function closeMobileDetail() {
		selectionVersion += 1;
		const returnId = effectiveSelectedId;
		mobileDetailOpen = false;
		decliningId = undefined;
		syncUrl({ item: null });
		void tick().then(() => {
			if (destroyed) return;
			const row = returnId
				? document.querySelector<HTMLElement>(`[data-triage-row="${CSS.escape(returnId)}"]`)
				: null;
			focusElementFallback(row, searchInputEl, document.getElementById('shell-page-title'));
		});
	}

	function setScope(nextScope: InboxScope, preserveSelection = false) {
		selectionVersion += 1;
		scope = nextScope;
		decliningId = undefined;
		const keepSelected =
			preserveSelection && selectedItem !== undefined && matchesScope(selectedItem, nextScope);
		if (keepSelected && selectedItem) {
			selectedId = selectedItem.id;
			syncUrl({
				team: nextScope.team ?? null,
				project: nextScope.project ?? null,
				item: selectedItem.id
			});
			return;
		}
		const wasMobileDetail = showingMobileDetail;
		mobileDetailOpen = false;
		syncUrl({ team: nextScope.team ?? null, project: nextScope.project ?? null, item: null });
		if (wasMobileDetail) {
			void tick().then(() => {
				if (destroyed) return;
				focusElementFallback(
					document.querySelector<HTMLElement>('[data-triage-row]'),
					searchInputEl,
					document.getElementById('shell-page-title')
				);
			});
		}
	}

	function resetFilters() {
		selectionVersion += 1;
		activeTab = 'all';
		searchQuery = '';
		urgentOnly = false;
		scope = {};
		decliningId = undefined;
		mobileDetailOpen = false;
		syncUrl({ tab: 'all', item: null, team: null, project: null });
	}

	async function navigateToResolved(rawHref: string) {
		const { pathname, suffix } = splitHref(rawHref);
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- pathname passes through resolve() while dynamic ?query/#hash suffix stays verbatim
		await goto(`${resolve(pathname as '/work')}${suffix}`);
	}

	async function completeAndAdvance(
		ctx: {
			token: number;
			targetOrg: string;
			startSelVersion: number;
			startHref: string;
			candidateIds: readonly string[];
		},
		resolvedId: string,
		sourceKind: TriageItem['sourceKind'],
		message: string,
		clearDrafts: boolean
	) {
		if (clearDrafts) drafts.clearActionDrafts(ctx.targetOrg, resolvedId);
		if (organizationId === ctx.targetOrg && decliningId === resolvedId) decliningId = undefined;
		if (message) toast.success(message);
		if (!isCurrentAction(ctx.token, ctx.targetOrg)) return;
		const refreshed = await onRefresh();
		if (!isCurrentAction(ctx.token, ctx.targetOrg) || !isTriageSourceFresh(sourceKind, refreshed)) {
			return;
		}
		nowMs = Date.now();
		await tick();
		if (
			!isCurrentAction(ctx.token, ctx.targetOrg) ||
			selectionVersion !== ctx.startSelVersion ||
			page.url.href !== ctx.startHref ||
			navigating.to ||
			visibleItems.some((i) => i.id === resolvedId)
		) {
			return;
		}
		const nextId = selectNextVisibleAfterResolve(
			visibleItems,
			resolvedId,
			ctx.candidateIds,
			refreshed,
			wide.current
		);
		selectedId = nextId;
		if (!wide.current) {
			if (nextId && mobileDetailOpen) syncUrl({ item: nextId });
			else {
				mobileDetailOpen = false;
				syncUrl({ item: null });
				await tick();
				if (!destroyed && !isLeavingInbox()) {
					focusElementFallback(searchInputEl, document.getElementById('shell-page-title'));
				}
			}
			return;
		}
		syncUrl({ item: nextId ?? null });
	}

	async function runItemAction(
		item: TriageItem,
		execute: (targetOrg: string) => Promise<TriageActionOutcome>,
		formatError: (msg: string) => string = (m) => m
	): Promise<TriageActionResult> {
		const token = ++actionSeq;
		const targetOrg = organizationId;
		const ctx = {
			token,
			targetOrg,
			startSelVersion: selectionVersion,
			startHref: pendingUrl?.href ?? page.url.href,
			candidateIds: neighborCandidatesAfterResolve(visibleItems, item.id)
		};
		activeAction = { token, orgId: targetOrg, itemId: item.id };
		try {
			const outcome = await execute(targetOrg);
			if (outcome.kind === 'navigate') {
				if (
					isCurrentAction(token, targetOrg) &&
					page.url.href === ctx.startHref &&
					!navigating.to
				) {
					await navigateToResolved(outcome.href);
				}
			} else if (outcome.kind === 'refresh') {
				toast.success(outcome.message);
				if (isCurrentAction(token, targetOrg)) {
					await onRefresh();
					nowMs = Date.now();
				}
			} else if (outcome.kind === 'advance') {
				await completeAndAdvance(
					ctx,
					item.id,
					item.sourceKind,
					outcome.message,
					outcome.clearDrafts
				);
			}
			return { ok: true };
		} catch (e) {
			const msg = formatError(toAppError(e).message);
			toast.error(msg);
			return { ok: false, error: msg };
		} finally {
			if (activeAction?.token === token) activeAction = undefined;
		}
	}

	const actions = createTriageActionHandlers({
		isLocked: () => actionLocked,
		getOrgId: () => organizationId,
		getSnapshot: () => snapshot,
		getZone: () => accountZone.zone,
		getDecliningId: () => decliningId,
		setDecliningId: (id) => {
			decliningId = id;
		},
		getDenyDraft: (orgId, itemId) => drafts.getDeny(orgId, itemId),
		onError: (msg) => toast.error(msg),
		runItemAction,
		formatSnoozeError
	});

	function openCustomSnoozeDialog(item: TriageItem, from: HTMLElement | null) {
		snoozeOpener = from;
		snoozeDialog?.start(item);
	}

	function restoreSnoozeFocus(event: Event) {
		event.preventDefault();
		focusElementFallback(
			snoozeOpener,
			document.getElementById('triage-detail-heading'),
			document.querySelector<HTMLElement>('[data-triage-row]'),
			searchInputEl,
			document.getElementById('shell-page-title')
		);
	}

	function dismissMobileDetailIfOpen() {
		if (!showingMobileDetail) return;
		selectionVersion += 1;
		mobileDetailOpen = false;
		syncUrl({ item: null });
	}

	function workspaceShortcuts(node: HTMLElement) {
		return attachWorkspaceShortcuts(node, {
			shortcutsEnabled: () => shortcutsEnabled,
			denyGuardVisible: () => denyGuardVisible,
			selectedItem: () => selectedItem,
			actionLocked: () => actionLocked,
			onOpenSearch: () => {
				dismissMobileDetailIfOpen();
				focusSearchInput();
			},
			onNavigateRelative: (delta) => {
				const nextId = computeRelativeSelectionId(visibleItems, effectiveSelectedId, delta);
				if (nextId) selectItem(nextId);
			},
			onApprove: (item) => void actions.primary(item),
			onArmDeny: (itemId) => {
				decliningId = itemId;
			},
			onSelectTab: selectTab,
			onShowCategories: dismissMobileDetailIfOpen
		});
	}
</script>

<div
	class={[
		'inbox-page triage-page flex w-full flex-col',
		showingMobileDetail && 'triage-has-detail'
	]}
>
	<section
		{@attach workspaceShortcuts}
		aria-label="Inbox workspace"
		data-inbox-dividers={showDividers ? '' : undefined}
		class="triage-workspace flex min-h-0 w-full flex-1 flex-col overflow-hidden"
	>
		<InboxTriageHeader
			{totalOpenCount}
			{viewingNotifications}
			{activeTab}
			{counts}
			{notifTotalCount}
			{notifAttentionCount}
			{shortcutsEnabled}
			{searchOpen}
			{searchQuery}
			{urgentOnly}
			{scope}
			{scopeOpts}
			{sortOrder}
			{showDividers}
			{isFiltered}
			{displayActive}
			bind:searchInputEl
			onSelectTab={selectTab}
			onSelectNotificationsTab={selectNotificationsTab}
			onToggleInlineSearch={toggleInlineSearch}
			onCloseInlineSearch={closeInlineSearch}
			onSearchQueryChange={(val) => {
				selectionVersion += 1;
				searchQuery = val;
			}}
			onSetUrgentOnly={(next) => {
				selectionVersion += 1;
				urgentOnly = next;
				decliningId = undefined;
			}}
			onSetScope={(nextScope) => setScope(nextScope)}
			onClearFilterMenu={() => {
				selectionVersion += 1;
				urgentOnly = false;
				setScope({});
			}}
			onSetSortOrder={(next) => {
				selectionVersion += 1;
				sortOrder = next;
			}}
			onToggleRowDividers={(checked) => {
				showDividers = checked;
				writeInboxDividersPreference(checked);
			}}
		/>

		{#if viewingNotifications}
			<InboxNotificationsSplit {searchQuery} {nowMs} />
		{:else if loading}
			<InboxTriageSkeleton />
		{:else}
			<div class="triage-split min-h-0 flex-1">
				<InboxQueueList
					{groups}
					visibleCount={visibleItems.length}
					{totalOpenCount}
					{activeTab}
					{sortOrder}
					selectedId={effectiveSelectedId}
					{busyId}
					{searchQuery}
					{hasActiveFilters}
					zone={accountZone.zone}
					{nowMs}
					snoozedError={snoozedLoadError}
					onRetrySnoozed={() => void onRefresh()}
					onSelect={selectItem}
					onResetFilters={resetFilters}
				/>
				<InboxDetailPane
					{organizationId}
					item={selectedItem}
					{searchQuery}
					{busyId}
					{actionLocked}
					{shortcutsEnabled}
					{decliningId}
					declineReason={activeDeclineReason}
					replyText={activeReplyText}
					commentDraft={activeCommentDraft}
					zone={accountZone.zone}
					{nowMs}
					onDeclineReasonChange={(id, val) => drafts.setDeny(organizationId, id, val)}
					onReplyTextChange={(id, val) => drafts.setReply(organizationId, id, val)}
					onCommentDraftChange={(id, val) => drafts.setComment(organizationId, id, val)}
					onCommentDraftClear={(orgId, id) => drafts.clearComment(orgId, id)}
					onBackMobile={closeMobileDetail}
					onPrimaryAction={(it) => void actions.primary(it)}
					onSecondaryAction={(it) => void actions.secondary(it)}
					onArmDeny={(id) => {
						decliningId = id;
					}}
					onCancelDeny={() => {
						decliningId = undefined;
					}}
					onChoose={(it, choiceId, label) => void actions.choose(it, choiceId, label)}
					onReply={(it, text) => void actions.reply(it, text)}
					onDefer={(it, untilIso, note) => void actions.defer(it, untilIso, note)}
					onOpenCustomSnooze={openCustomSnoozeDialog}
					onUnsnooze={(it) => void actions.unsnooze(it)}
				/>
			</div>
		{/if}
	</section>
</div>

<InboxSnoozeDialog
	bind:this={snoozeDialog}
	zone={accountZone.zone}
	busy={actionLocked}
	onsubmit={actions.defer}
	onCloseAutoFocus={restoreSnoozeFocus}
/>

<style>
	.triage-workspace {
		--triage-gutter: 24px;
		--triage-control-height: 36px;
		min-height: min(680px, 100dvh);
	}
	@media (min-width: 960px) and (min-height: 640px) {
		.triage-workspace {
			height: 100%;
			min-height: 0;
		}
	}
	@media (min-width: 960px) and (max-height: 639px) {
		.triage-workspace {
			min-height: 600px;
		}
	}
	@media (max-width: 720px), (pointer: coarse) {
		.triage-workspace {
			--triage-gutter: 16px;
			--triage-control-height: 44px;
		}
	}
	.triage-split {
		display: grid;
		grid-template-columns: clamp(19rem, 34%, 31rem) minmax(0, 1fr);
		grid-template-rows: minmax(0, 1fr);
		min-height: 0;
	}
	@media (max-width: 959px) {
		.triage-split {
			display: block;
		}
		.triage-page:not(.triage-has-detail) :global(.triage-reading) {
			display: none;
		}
		.triage-has-detail :global(.triage-toolbar),
		.triage-has-detail :global(.triage-queue) {
			display: none;
		}
		.triage-has-detail :global(.queue-headline) {
			padding-block-end: 12px;
		}
	}
</style>
