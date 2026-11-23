<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { MediaQuery } from 'svelte/reactivity';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { navigating, page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import ListFilter from '@lucide/svelte/icons/list-filter';
	import Search from '@lucide/svelte/icons/search';
	import SlidersHorizontal from '@lucide/svelte/icons/sliders-horizontal';
	import X from '@lucide/svelte/icons/x';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { executeDecisionAction } from '$lib/home/actions';
	import { isDenyAction } from '$lib/home/model';
	import { toAppError } from '$lib/office/api';
	import { isEditableTarget, singleKeyShortcutsOn } from '$lib/platform/shell/shortcuts';
	import type { ShellRefreshResult } from '$lib/platform/shell/shell-data.svelte';
	import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import { inboxApi } from '../../api';
	import type { InboxItem, InboxScope } from '../../types';
	import InboxDetailPane from './InboxDetailPane.svelte';
	import InboxQueueList from './InboxQueueList.svelte';
	import {
		deriveTriageItems,
		filterTriageItems,
		flattenGroups,
		groupTriageItems,
		INBOX_DIVIDERS_STORAGE_KEY,
		isOverflowTab,
		matchesScope,
		neighborCandidatesAfterResolve,
		openWaitingCount,
		OVERFLOW_TABS,
		parseTriageTab,
		PRIMARY_TABS,
		splitHref,
		triageScopeOptions,
		triageTabCounts,
		type TriageItem,
		type TriageOrder,
		type TriageTab
	} from './triage-model';

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

	function safeReadDividers(): boolean {
		if (typeof window === 'undefined') return false;
		try {
			return window.localStorage.getItem(INBOX_DIVIDERS_STORAGE_KEY) === '1';
		} catch {
			return false;
		}
	}

	function safeWriteDividers(enabled: boolean): void {
		if (typeof window === 'undefined') return;
		try {
			window.localStorage.setItem(INBOX_DIVIDERS_STORAGE_KEY, enabled ? '1' : '0');
		} catch {
			// Ignore private mode / quota failures cleanly.
		}
	}

	const wide = new MediaQuery('min-width: 960px');

	// Writable derived states stay synced with URL query params and org changes without $effect state writes.
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
	let urgentOnly = $state(false);
	let sortOrder = $state<TriageOrder>('priority');
	let showDividers = $state(safeReadDividers());

	// Monotonic operation token prevents stale in-flight actions from clearing newer busy/selection state.
	let actionSeq = 0;
	let activeAction = $state<{ token: number; orgId: string; itemId: string } | undefined>(
		undefined
	);
	const actionLocked = $derived(Boolean(activeAction && activeAction.orgId === organizationId));
	const busyId = $derived(
		activeAction && activeAction.orgId === organizationId ? activeAction.itemId : undefined
	);
	const shortcutsEnabled = $derived(singleKeyShortcutsOn());

	// Monotonic selection version incremented only on explicit user navigation/selection changes.
	let selectionVersion = 0;

	let decliningId = $derived.by<string | undefined>(() => {
		void organizationId;
		return undefined;
	});
	// Keyed by `${organizationId}:${itemId}` so drafts never leak across items or organizations.
	let denyDrafts = $state<Record<string, string>>({});
	let replyDrafts = $state<Record<string, string>>({});
	let commentDrafts = $state<Record<string, string>>({});
	let nowMs = $state(Date.now());
	let searchInputEl = $state<HTMLInputElement | null>(null);

	// Composes overlapping syncUrl() updates while an earlier goto() is still settling.
	let pendingUrl: URL | undefined;
	let pendingBaseHref = '';
	let navSeq = 0;

	// Track mount lifetime and pending cross-route navigations so late actions never hijack outgoing routes.
	const inboxPathname = resolve('/inbox');
	let destroyed = false;
	onDestroy(() => {
		destroyed = true;
	});

	const isLeavingInbox = () =>
		page.url.pathname !== inboxPathname ||
		Boolean(navigating.to && navigating.to.url.pathname !== inboxPathname);

	const draftKey = (orgId: string, itemId: string) => `${orgId}:${itemId}`;
	const isCurrentAction = (token: number, targetOrg: string) =>
		!destroyed &&
		!isLeavingInbox() &&
		activeAction?.token === token &&
		organizationId === targetOrg;

	const allItems = $derived(deriveTriageItems(workItems, snapshot, ownerId, inboxItems, nowMs));
	const totalOpenCount = $derived(openWaitingCount(allItems));
	const scopeOpts = $derived.by(() => {
		const fromItems = triageScopeOptions(allItems);
		const teams: Record<string, string> = {};
		for (const t of fromItems.teams) teams[t.id] = t.name;
		for (const t of snapshot.teams ?? []) {
			if (!Object.hasOwn(teams, t.id)) teams[t.id] = t.name;
		}
		const projects: Record<string, string> = {};
		for (const p of fromItems.projects) projects[p.id] = p.name;
		for (const p of snapshot.projects ?? []) {
			if (!p.archived && !Object.hasOwn(projects, p.id)) projects[p.id] = p.name;
		}
		const toSorted = (record: Record<string, string>) =>
			Object.entries(record)
				.map(([id, name]) => ({ id, name }))
				.sort((a, b) => a.name.localeCompare(b.name));
		return { teams: toSorted(teams), projects: toSorted(projects) };
	});
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
	const overflowActive = $derived(isOverflowTab(activeTab));
	const overflowActiveLabel = $derived(
		OVERFLOW_TABS.find((t) => t.id === activeTab)?.label ?? 'More'
	);

	// Hand off focus to the detail heading whenever mobile detail opens (including ?item= deep links).
	// Record lastFocusedMobileId only after focus() runs so a same-ID refresh before tick() never skips focus.
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

	const activeDeclineReason = $derived(
		selectedItem ? (denyDrafts[draftKey(organizationId, selectedItem.id)] ?? '') : ''
	);
	const activeReplyText = $derived(
		selectedItem ? (replyDrafts[draftKey(organizationId, selectedItem.id)] ?? '') : ''
	);
	const activeCommentDraft = $derived(
		selectedItem ? (commentDrafts[draftKey(organizationId, selectedItem.id)] ?? '') : ''
	);

	function updateDenyDraft(itemId: string, value: string) {
		denyDrafts = { ...denyDrafts, [draftKey(organizationId, itemId)]: value };
	}

	function updateReplyDraft(itemId: string, value: string) {
		replyDrafts = { ...replyDrafts, [draftKey(organizationId, itemId)]: value };
	}

	function updateCommentDraft(itemId: string, value: string) {
		commentDrafts = { ...commentDrafts, [draftKey(organizationId, itemId)]: value };
	}

	function clearCommentDraft(orgId: string, itemId: string) {
		const key = draftKey(orgId, itemId);
		if (key in commentDrafts) {
			const next = { ...commentDrafts };
			delete next[key];
			commentDrafts = next;
		}
	}

	function clearItemDrafts(orgId: string, itemId: string) {
		const key = draftKey(orgId, itemId);
		if (key in denyDrafts) {
			const next = { ...denyDrafts };
			delete next[key];
			denyDrafts = next;
		}
		if (key in replyDrafts) {
			const next = { ...replyDrafts };
			delete next[key];
			replyDrafts = next;
		}
	}

	function syncUrl(next: {
		tab?: TriageTab;
		item?: string | null;
		team?: string | null;
		project?: string | null;
	}) {
		if (destroyed || isLeavingInbox()) return;
		const liveHref = page.url.href;
		const hasPending = Boolean(pendingUrl && liveHref === pendingBaseHref);
		const url = hasPending && pendingUrl ? new URL(pendingUrl) : new URL(page.url);
		pendingBaseHref = liveHref;

		const nextTab = next.tab ?? activeTab;
		if (nextTab === 'all') url.searchParams.delete('tab');
		else url.searchParams.set('tab', nextTab);

		if ('item' in next) {
			if (next.item) url.searchParams.set('item', next.item);
			else url.searchParams.delete('item');
		}
		if ('team' in next) {
			if (next.team) url.searchParams.set('team', next.team);
			else url.searchParams.delete('team');
		}
		if ('project' in next) {
			if (next.project) url.searchParams.set('project', next.project);
			else url.searchParams.delete('project');
		}

		// Supersede pending navigation even when target matches current page.url (e.g., immediate Back).
		const shouldNavigate = hasPending ? pendingUrl?.href !== url.href : url.href !== liveHref;

		// Preserve cloned URL query/hash without passing dynamic fragments through resolve().
		if (shouldNavigate) {
			const seq = ++navSeq;
			pendingUrl = new URL(url);
			// eslint-disable-next-line svelte/no-navigation-without-resolve -- cloned page.url preserves deployment base path and arbitrary query/hash fragments
			void goto(url, {
				replaceState: true,
				keepFocus: true,
				noScroll: true
			}).finally(() => {
				if (seq === navSeq) pendingUrl = undefined;
			});
		}
	}

	function selectTab(tab: TriageTab) {
		selectionVersion += 1;
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

	function selectItem(id: string) {
		selectionVersion += 1;
		selectedId = id;
		decliningId = undefined;
		if (!wide.current) {
			mobileDetailOpen = true;
		}
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
			(row ?? searchInputEl ?? document.getElementById('shell-page-title'))?.focus();
		});
	}

	function setSearchQuery(value: string) {
		selectionVersion += 1;
		searchQuery = value;
	}

	function setUrgentOnly(next: boolean) {
		selectionVersion += 1;
		urgentOnly = next;
		decliningId = undefined;
	}

	function setSortOrder(next: TriageOrder) {
		selectionVersion += 1;
		sortOrder = next;
	}

	function toggleRowDividers(checked: boolean) {
		showDividers = checked;
		safeWriteDividers(checked);
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
		syncUrl({
			team: nextScope.team ?? null,
			project: nextScope.project ?? null,
			item: null
		});
		if (wasMobileDetail) {
			void tick().then(() => {
				if (destroyed) return;
				const firstRow = document.querySelector<HTMLElement>('[data-triage-row]');
				(firstRow ?? searchInputEl ?? document.getElementById('shell-page-title'))?.focus();
			});
		}
	}

	function clearFilterMenu() {
		selectionVersion += 1;
		urgentOnly = false;
		setScope({});
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
		token: number,
		targetOrg: string,
		resolvedId: string,
		sourceKind: TriageItem['sourceKind'],
		startSelVersion: number,
		startHref: string,
		candidateNeighborIds: readonly string[],
		message: string
	) {
		// Clear draft and armed Deny guard immediately once the mutation succeeds, even if refresh fails.
		clearItemDrafts(targetOrg, resolvedId);
		if (organizationId === targetOrg && decliningId === resolvedId) decliningId = undefined;
		if (message) toast.success(message);

		// Verify route lifetime, active token, and org match before refreshing.
		if (!isCurrentAction(token, targetOrg)) return;
		const refreshed = await onRefresh();
		if (!isCurrentAction(token, targetOrg)) return;

		// Only advance selection when the owning data source actually published fresh rows.
		const isSourceFresh = (kind: TriageItem['sourceKind']) =>
			kind === 'inbox' ? refreshed.inboxUpdated : refreshed.workUpdated;
		if (!isSourceFresh(sourceKind)) return;

		nowMs = Date.now();
		await tick();

		// Guard: only mutate live UI state if action is current, URL hasn't changed (e.g., Browser Back), and user hasn't navigated/filtered.
		if (!isCurrentAction(token, targetOrg)) return;
		if (selectionVersion !== startSelVersion || page.url.href !== startHref || navigating.to)
			return;
		if (visibleItems.some((i) => i.id === resolvedId)) return;

		// Skip candidate neighbors whose own backing source failed to publish fresh data.
		const surviving = visibleItems.filter(
			(i) => i.id !== resolvedId && isSourceFresh(i.sourceKind)
		);
		const refreshedIds = new Set(surviving.map((i) => i.id));
		const nextId =
			candidateNeighborIds.find((id) => refreshedIds.has(id)) ??
			(wide.current ? surviving[0]?.id : undefined);

		selectedId = nextId;
		if (!wide.current) {
			if (nextId && mobileDetailOpen) {
				syncUrl({ item: nextId });
			} else {
				mobileDetailOpen = false;
				syncUrl({ item: null });
				await tick();
				if (!destroyed && !isLeavingInbox()) {
					(searchInputEl ?? document.getElementById('shell-page-title'))?.focus();
				}
			}
			return;
		}
		syncUrl({ item: nextId ?? null });
	}

	async function runPrimaryAction(item: TriageItem) {
		if (actionLocked || !item.decision) return;
		const token = ++actionSeq;
		const targetOrg = organizationId;
		const startSelVersion = selectionVersion;
		const startHref = pendingUrl?.href ?? page.url.href;
		const candidateIds = neighborCandidatesAfterResolve(visibleItems, item.id);
		const reason = denyDrafts[draftKey(targetOrg, item.id)] ?? '';
		activeAction = { token, orgId: targetOrg, itemId: item.id };
		try {
			const res = await executeDecisionAction(
				targetOrg,
				item.decision,
				'primary',
				snapshot,
				reason
			);
			if (res.navigateTo) {
				if (!isCurrentAction(token, targetOrg) || page.url.href !== startHref || navigating.to)
					return;
				await navigateToResolved(res.navigateTo);
				return;
			}
			await completeAndAdvance(
				token,
				targetOrg,
				item.id,
				item.sourceKind,
				startSelVersion,
				startHref,
				candidateIds,
				res.message
			);
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			if (activeAction?.token === token) activeAction = undefined;
		}
	}

	async function runSecondaryAction(item: TriageItem) {
		if (actionLocked) return;
		const targetOrg = organizationId;
		const canDenyItem =
			item.inboxItem?.actions.includes('deny') || isDenyAction(item.decision?.secondaryAction);
		if (canDenyItem && decliningId !== item.id) {
			decliningId = item.id;
			return;
		}

		const token = ++actionSeq;
		const startSelVersion = selectionVersion;
		const startHref = pendingUrl?.href ?? page.url.href;
		const candidateIds = neighborCandidatesAfterResolve(visibleItems, item.id);
		const reason = denyDrafts[draftKey(targetOrg, item.id)] ?? '';
		activeAction = { token, orgId: targetOrg, itemId: item.id };
		try {
			if (item.decision && isDenyAction(item.decision.secondaryAction)) {
				const res = await executeDecisionAction(
					targetOrg,
					item.decision,
					'secondary',
					snapshot,
					reason
				);
				if (res.navigateTo) {
					if (!isCurrentAction(token, targetOrg) || page.url.href !== startHref || navigating.to)
						return;
					await navigateToResolved(res.navigateTo);
					return;
				}
				await completeAndAdvance(
					token,
					targetOrg,
					item.id,
					item.sourceKind,
					startSelVersion,
					startHref,
					candidateIds,
					res.message
				);
			} else if (item.inboxItem?.actions.includes('deny')) {
				const finalReason = reason.trim() || 'Declined from executive review';
				await inboxApi.act(targetOrg, item.inboxItem.id, { type: 'deny', reason: finalReason });
				await completeAndAdvance(
					token,
					targetOrg,
					item.id,
					item.sourceKind,
					startSelVersion,
					startHref,
					candidateIds,
					`Denied “${item.inboxItem.title}”.`
				);
			}
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			if (activeAction?.token === token) activeAction = undefined;
		}
	}

	async function runChooseAction(item: TriageItem, choiceId: string, choiceLabel: string) {
		if (actionLocked || !item.inboxItem) return;
		const token = ++actionSeq;
		const targetOrg = organizationId;
		const startSelVersion = selectionVersion;
		const startHref = pendingUrl?.href ?? page.url.href;
		const candidateIds = neighborCandidatesAfterResolve(visibleItems, item.id);
		activeAction = { token, orgId: targetOrg, itemId: item.id };
		try {
			await inboxApi.act(targetOrg, item.inboxItem.id, { type: 'choose', choice: choiceId });
			await completeAndAdvance(
				token,
				targetOrg,
				item.id,
				item.sourceKind,
				startSelVersion,
				startHref,
				candidateIds,
				`Recorded “${choiceLabel}”.`
			);
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			if (activeAction?.token === token) activeAction = undefined;
		}
	}

	async function runReplyAction(item: TriageItem, text: string) {
		if (actionLocked || !item.inboxItem || !text.trim()) return;
		const token = ++actionSeq;
		const targetOrg = organizationId;
		const startSelVersion = selectionVersion;
		const startHref = pendingUrl?.href ?? page.url.href;
		const candidateIds = neighborCandidatesAfterResolve(visibleItems, item.id);
		activeAction = { token, orgId: targetOrg, itemId: item.id };
		try {
			await inboxApi.act(targetOrg, item.inboxItem.id, { type: 'reply', text: text.trim() });
			await completeAndAdvance(
				token,
				targetOrg,
				item.id,
				item.sourceKind,
				startSelVersion,
				startHref,
				candidateIds,
				`Sent reply to ${item.actorName}.`
			);
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			if (activeAction?.token === token) activeAction = undefined;
		}
	}

	function navigateRelative(delta: number) {
		if (visibleItems.length === 0) return;
		const currentIdx = visibleItems.findIndex((i) => i.id === effectiveSelectedId);
		const startIdx = currentIdx < 0 ? 0 : currentIdx;
		const nextIdx = Math.max(0, Math.min(visibleItems.length - 1, startIdx + delta));
		const next = visibleItems[nextIdx];
		if (next) selectItem(next.id);
	}

	function handleWorkspaceKeydown(event: KeyboardEvent) {
		if (
			event.defaultPrevented ||
			event.isComposing ||
			event.metaKey ||
			event.ctrlKey ||
			event.altKey ||
			event.shiftKey
		) {
			return;
		}
		if (document.querySelector('[role="dialog"], [role="menu"]')) return;
		if (isEditableTarget(event.target)) return;

		// Always stop unmodified `/` from bubbling to ExecutiveCommandBar before repeat/deny/pref early returns.
		if (event.key === '/') {
			event.stopPropagation();
			if (!shortcutsEnabled || event.repeat || denyGuardVisible) return;
			event.preventDefault();
			if (showingMobileDetail) {
				selectionVersion += 1;
				mobileDetailOpen = false;
				syncUrl({ item: null });
			}
			void tick().then(() => {
				if (!destroyed) searchInputEl?.focus();
			});
			return;
		}

		if (!shortcutsEnabled || event.repeat || denyGuardVisible) return;

		if (event.key === 'j') {
			event.preventDefault();
			navigateRelative(1);
		} else if (event.key === 'k') {
			event.preventDefault();
			navigateRelative(-1);
		} else if (event.key === 'a' && selectedItem?.state === 'open' && !actionLocked) {
			const primaryKind = selectedItem.decision?.primaryAction.kind;
			if (primaryKind === 'approve_inbox' || primaryKind === 'approve_request') {
				event.preventDefault();
				void runPrimaryAction(selectedItem);
			}
		} else if (event.key === 'd' && selectedItem?.state === 'open' && !actionLocked) {
			const canDenySelected =
				selectedItem.inboxItem?.actions.includes('deny') ||
				isDenyAction(selectedItem.decision?.secondaryAction);
			if (canDenySelected) {
				event.preventDefault();
				decliningId = selectedItem.id;
			}
		}
	}

	// Scope single-character shortcuts strictly to the triage workspace container (WCAG 2.1.4).
	function workspaceShortcuts(node: HTMLElement) {
		const listener = (event: KeyboardEvent) => handleWorkspaceKeydown(event);
		node.addEventListener('keydown', listener);
		return () => node.removeEventListener('keydown', listener);
	}
</script>

<div
	class={[
		'inbox-page triage-page flex w-full flex-col px-4 pt-2 pb-6 sm:px-6',
		showingMobileDetail && 'triage-has-detail'
	]}
>
	<section
		{@attach workspaceShortcuts}
		aria-label="Inbox workspace"
		data-inbox-dividers={showDividers ? '' : undefined}
		class="exec-card triage-workspace flex min-h-0 w-full flex-col overflow-hidden"
	>
		<!-- Header & Controls (poc/operations.css queue-header) -->
		<header class="inbox-head triage-header queue-header">
			<div class="queue-headline">
				<div class="ph-t">
					<h1 id="shell-page-title" tabindex="-1" class="triage-h1">Inbox</h1>
					<span role="status" class="queue-count">
						<i class={[totalOpenCount === 0 && 'is-ok']} aria-hidden="true"></i>
						<span class="tabular-nums">
							{totalOpenCount === 0 ? '0 waiting on you' : `${totalOpenCount} waiting on you`}
						</span>
					</span>
				</div>

				<div class="queue-search-wrap">
					<div class="triage-search">
						<Search class="triage-search-ico size-4 shrink-0" aria-hidden="true" />
						<label class="sr-only" for="triage-search-input">Search inbox</label>
						<input
							bind:this={searchInputEl}
							value={searchQuery}
							oninput={(e) => setSearchQuery(e.currentTarget.value)}
							id="triage-search-input"
							type="text"
							autocomplete="off"
							placeholder="Search inbox…"
							class="triage-search-input"
						/>
						{#if searchQuery}
							<button
								type="button"
								onclick={() => {
									setSearchQuery('');
									searchInputEl?.focus();
								}}
								aria-label="Clear search"
								class="triage-clear-btn inline-flex min-h-9 min-w-9 items-center justify-center max-[720px]:min-h-11 max-[720px]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
							>
								<X class="size-3.5" aria-hidden="true" />
							</button>
						{:else if shortcutsEnabled}
							<kbd class="triage-search-kbd" aria-hidden="true">/</kbd>
						{/if}
					</div>
				</div>
			</div>

			<!-- 3 Primary Categories + More dropdown + Filter & Display icon menus -->
			<div class="triage-toolbar queue-toolbar">
				<div role="group" aria-label="Inbox categories" class="queue-tabs">
					{#each PRIMARY_TABS as tab (tab.id)}
						{@const isSelected = activeTab === tab.id}
						<button
							type="button"
							aria-pressed={isSelected}
							onclick={() => selectTab(tab.id)}
							class="queue-tab"
						>
							<span>{tab.label}</span>
							<span class="queue-tab-count">{counts[tab.id]}</span>
						</button>
					{/each}

					<div class="triage-tab-tools">
						<DropdownMenu.Root>
							<DropdownMenu.Trigger>
								{#snippet child({ props })}
									<button
										{...props}
										type="button"
										data-current-kind={overflowActive ? '' : undefined}
										title={overflowActive ? `Viewing ${overflowActiveLabel}` : undefined}
										class="queue-tab"
									>
										<span>More</span>
										<ChevronDown class="size-3.5 shrink-0 opacity-75" aria-hidden="true" />
									</button>
								{/snippet}
							</DropdownMenu.Trigger>
							<DropdownMenu.Content
								preventScroll={false}
								align="start"
								class="w-52 motion-reduce:animate-none! motion-reduce:transition-none! motion-reduce:data-[state=closed]:animate-none! motion-reduce:data-[state=open]:animate-none!"
							>
								<DropdownMenu.RadioGroup
									value={overflowActive ? activeTab : ''}
									onValueChange={(val) => {
										if (val) selectTab(val as TriageTab);
									}}
								>
									{#each OVERFLOW_TABS as tab (tab.id)}
										<DropdownMenu.RadioItem
											value={tab.id}
											closeOnSelect={true}
											class="flex min-h-9 items-center justify-between gap-3 pr-8 max-[720px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
										>
											<span>{tab.label}</span>
											<span class="triage-menu-count font-mono text-xs tabular-nums">
												{counts[tab.id]}
											</span>
										</DropdownMenu.RadioItem>
									{/each}
								</DropdownMenu.RadioGroup>
							</DropdownMenu.Content>
						</DropdownMenu.Root>

						<div class="queue-controls" role="group" aria-label="Inbox view controls">
							<DropdownMenu.Root>
								<DropdownMenu.Trigger>
									{#snippet child({ props })}
										<button
											{...props}
											type="button"
											aria-label="Filter inbox"
											title="Filter inbox"
											data-active={isFiltered ? '' : undefined}
											class="queue-icon-btn"
										>
											<ListFilter class="size-4" aria-hidden="true" />
										</button>
									{/snippet}
								</DropdownMenu.Trigger>
								<DropdownMenu.Content
									preventScroll={false}
									align="end"
									class="max-h-[min(60dvh,28rem)] w-60 max-w-[calc(100vw-2rem)] overflow-y-auto motion-reduce:animate-none! motion-reduce:transition-none! motion-reduce:data-[state=closed]:animate-none! motion-reduce:data-[state=open]:animate-none!"
								>
									<DropdownMenu.Label>Urgency</DropdownMenu.Label>
									<DropdownMenu.RadioGroup
										value={urgentOnly ? 'urgent' : 'all'}
										onValueChange={(val) => setUrgentOnly(val === 'urgent')}
									>
										<DropdownMenu.RadioItem
											value="all"
											closeOnSelect={true}
											class="min-h-9 max-[720px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
										>
											All items
										</DropdownMenu.RadioItem>
										<DropdownMenu.RadioItem
											value="urgent"
											closeOnSelect={true}
											class="min-h-9 max-[720px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
										>
											Urgent only
										</DropdownMenu.RadioItem>
									</DropdownMenu.RadioGroup>
									<DropdownMenu.Separator />
									<DropdownMenu.Label>Project</DropdownMenu.Label>
									<DropdownMenu.RadioGroup
										value={scope.project ?? ''}
										onValueChange={(val) => setScope({ ...scope, project: val || undefined })}
									>
										<DropdownMenu.RadioItem
											value=""
											closeOnSelect={true}
											class="min-h-9 max-[720px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
										>
											All projects
										</DropdownMenu.RadioItem>
										{#each scopeOpts.projects as proj (proj.id)}
											<DropdownMenu.RadioItem
												value={proj.id}
												closeOnSelect={true}
												class="min-h-9 max-[720px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
											>
												{proj.name}
											</DropdownMenu.RadioItem>
										{/each}
									</DropdownMenu.RadioGroup>
									<DropdownMenu.Separator />
									<DropdownMenu.Label>Team</DropdownMenu.Label>
									<DropdownMenu.RadioGroup
										value={scope.team ?? ''}
										onValueChange={(val) => setScope({ ...scope, team: val || undefined })}
									>
										<DropdownMenu.RadioItem
											value=""
											closeOnSelect={true}
											class="min-h-9 max-[720px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
										>
											All teams
										</DropdownMenu.RadioItem>
										{#each scopeOpts.teams as team (team.id)}
											<DropdownMenu.RadioItem
												value={team.id}
												closeOnSelect={true}
												class="min-h-9 max-[720px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
											>
												{team.name}
											</DropdownMenu.RadioItem>
										{/each}
									</DropdownMenu.RadioGroup>
									{#if isFiltered}
										<DropdownMenu.Separator />
										<DropdownMenu.Group>
											<DropdownMenu.Item
												onSelect={clearFilterMenu}
												class="min-h-9 max-[720px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
											>
												Reset filters
											</DropdownMenu.Item>
										</DropdownMenu.Group>
									{/if}
								</DropdownMenu.Content>
							</DropdownMenu.Root>

							<DropdownMenu.Root>
								<DropdownMenu.Trigger>
									{#snippet child({ props })}
										<button
											{...props}
											type="button"
											aria-label="Inbox display"
											title="Inbox display"
											data-active={displayActive ? '' : undefined}
											class="queue-icon-btn"
										>
											<SlidersHorizontal class="size-4" aria-hidden="true" />
										</button>
									{/snippet}
								</DropdownMenu.Trigger>
								<DropdownMenu.Content
									preventScroll={false}
									align="end"
									class="w-52 max-w-[calc(100vw-2rem)] motion-reduce:animate-none! motion-reduce:transition-none! motion-reduce:data-[state=closed]:animate-none! motion-reduce:data-[state=open]:animate-none!"
								>
									<DropdownMenu.Label>Grouping</DropdownMenu.Label>
									<DropdownMenu.RadioGroup
										value={sortOrder}
										onValueChange={(val) => {
											if (val === 'priority' || val === 'date') setSortOrder(val);
										}}
									>
										<DropdownMenu.RadioItem
											value="priority"
											closeOnSelect={true}
											class="min-h-9 max-[720px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
										>
											Priority first
										</DropdownMenu.RadioItem>
										<DropdownMenu.RadioItem
											value="date"
											closeOnSelect={true}
											class="min-h-9 max-[720px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
										>
											By date
										</DropdownMenu.RadioItem>
									</DropdownMenu.RadioGroup>
									<DropdownMenu.Separator />
									<DropdownMenu.CheckboxItem
										checked={showDividers}
										onCheckedChange={(checked) => toggleRowDividers(Boolean(checked))}
										class="min-h-9 max-[720px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
									>
										Row dividers
									</DropdownMenu.CheckboxItem>
								</DropdownMenu.Content>
							</DropdownMenu.Root>
						</div>
					</div>
				</div>
			</div>
		</header>

		<!-- Split Master-Detail Body -->
		{#if loading}
			<div class="triage-split flex-1" aria-busy="true" aria-label="Loading inbox">
				<div class="triage-skeleton-col flex flex-col gap-3 p-4">
					{#each [1, 2, 3, 4] as n (n)}
						<div class="triage-skel-row h-20"></div>
					{/each}
				</div>
				<div class="hidden flex-col gap-4 p-8 min-[960px]:flex">
					<div class="triage-skel-row h-10 w-64"></div>
					<div class="triage-skel-row h-24 w-full max-w-xl"></div>
					<div class="triage-skel-row h-32 w-full max-w-xl"></div>
				</div>
			</div>
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
					onDeclineReasonChange={updateDenyDraft}
					onReplyTextChange={updateReplyDraft}
					onCommentDraftChange={updateCommentDraft}
					onCommentDraftClear={clearCommentDraft}
					onBackMobile={closeMobileDetail}
					onPrimaryAction={(it) => void runPrimaryAction(it)}
					onSecondaryAction={(it) => void runSecondaryAction(it)}
					onArmDeny={(id) => {
						decliningId = id;
					}}
					onCancelDeny={() => {
						decliningId = undefined;
					}}
					onChoose={(it, choiceId, label) => void runChooseAction(it, choiceId, label)}
					onReply={(it, text) => void runReplyAction(it, text)}
				/>
			</div>
		{/if}
	</section>
</div>

<style>
	.triage-workspace {
		--triage-gutter: 24px;
		--triage-control-height: 36px;
		border-radius: var(--exec-panel-radius);
		min-height: min(680px, calc(100dvh - 104px));
	}
	/* Override .exec-card hover tint shift on the outer workspace shell */
	.triage-workspace:hover {
		background-color: var(--exec-surface);
		box-shadow: var(--exec-shadow);
	}
	@media (min-width: 960px) {
		.triage-workspace {
			height: max(600px, calc(100dvh - 104px));
		}
	}
	@media (max-width: 720px), (pointer: coarse) {
		.triage-workspace {
			--triage-gutter: 16px;
			--triage-control-height: 44px;
		}
	}
	.queue-header {
		position: relative;
		z-index: 1;
		container: queue-header / inline-size;
		border-bottom: 1px solid var(--exec-border);
	}
	.queue-headline {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: flex-start;
		gap: 8px 16px;
		padding: 14px var(--triage-gutter) 4px;
	}
	.ph-t {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 14px;
		min-width: 0;
	}
	.triage-h1 {
		color: var(--exec-ink);
		font-size: 1.25rem;
		line-height: 1.3;
		font-weight: 650;
		letter-spacing: -0.02em;
	}
	.triage-h1:focus {
		outline: none;
	}
	.triage-h1:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.queue-count {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		color: var(--exec-muted);
		font-size: 0.8125rem;
		font-weight: 500;
	}
	.queue-count i {
		display: inline-block;
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: var(--tone-accent);
		flex: none;
	}
	.queue-count i.is-ok {
		background: var(--tone-ok);
	}
	.queue-search-wrap {
		display: flex;
		align-items: center;
		min-width: 0;
		max-width: 100%;
		margin-inline: auto;
	}
	.triage-search {
		display: flex;
		align-items: center;
		gap: 8px;
		width: min(35rem, 60vw);
		max-width: 100%;
		min-width: 0;
		min-height: max(2.5rem, var(--triage-control-height));
		padding: 2px 8px 2px 10px;
		border: 1px solid var(--exec-border-hi);
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-well);
	}
	.triage-search:focus-within {
		border-color: var(--focus);
		outline: 2px solid var(--focus);
		outline-offset: -1px;
	}
	.triage-search-ico {
		color: var(--exec-muted);
	}
	.triage-search-input {
		min-width: 0;
		flex: 1 1 auto;
		width: 100%;
		min-height: var(--triage-control-height);
		border: 0;
		background: transparent;
		color: var(--exec-ink);
		font-size: 0.875rem;
		outline: none;
	}
	.triage-search-input::placeholder {
		color: var(--exec-muted);
		opacity: 1;
	}
	.triage-search-kbd {
		flex: none;
		padding: 1px 6px;
		border: 1px solid var(--exec-border-hi);
		border-radius: 4px;
		color: var(--exec-muted);
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.75rem;
		line-height: 1.3;
	}
	.triage-clear-btn {
		border-radius: var(--exec-sm-radius);
		color: var(--exec-muted);
		cursor: pointer;
	}
	.triage-clear-btn:hover {
		color: var(--exec-ink);
		background: var(--exec-btn-ghost-bg-hover);
	}
	.queue-toolbar {
		padding: 4px var(--triage-gutter) 0;
	}
	.queue-tabs {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 2px 4px;
	}
	.triage-tab-tools,
	.queue-controls {
		display: flex;
		align-items: center;
		gap: 4px;
	}
	.queue-tab {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 5px 12px;
		min-height: var(--triage-control-height);
		border: 1px solid transparent;
		border-radius: var(--exec-ctrl-radius) var(--exec-ctrl-radius) 0 0;
		background: transparent;
		color: var(--exec-body);
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.queue-tab:hover {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.queue-tab:is([aria-pressed='true'], [data-current-kind]) {
		border-bottom-color: var(--tone-accent-ink);
		box-shadow: inset 0 -1px var(--tone-accent-ink);
		color: var(--exec-ink);
	}
	.queue-tab[aria-expanded='true'] {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.queue-tab-count,
	.triage-menu-count {
		color: var(--exec-muted);
		font-variant-numeric: tabular-nums;
		font-size: 0.75rem;
	}
	.queue-tab[aria-pressed='true'] .queue-tab-count {
		color: var(--tone-accent-ink);
	}
	.queue-icon-btn {
		position: relative;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: var(--triage-control-height);
		min-height: var(--triage-control-height);
		border-radius: var(--exec-ctrl-radius);
		background: transparent;
		color: var(--exec-body);
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.queue-icon-btn:hover,
	.queue-icon-btn[aria-expanded='true'] {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.queue-icon-btn[data-active] {
		color: var(--tone-accent-ink);
	}
	.queue-icon-btn[data-active]::after {
		content: '';
		position: absolute;
		inset: 5px 5px auto auto;
		width: 5px;
		height: 5px;
		border-radius: 50%;
		background: currentColor;
	}
	.triage-split {
		display: grid;
		grid-template-columns: clamp(19rem, 34%, 31rem) minmax(0, 1fr);
		grid-template-rows: minmax(0, 1fr);
		min-height: 0;
	}
	.triage-skeleton-col {
		border-inline-end: 1px solid var(--exec-border);
	}
	.triage-skel-row {
		border-radius: var(--exec-inset-radius);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}

	@container queue-header (min-width: 52rem) {
		.queue-headline {
			display: grid;
			grid-template-columns: minmax(0, 1fr) min(35rem, 48%) minmax(0, 1fr);
		}
		.queue-search-wrap {
			grid-column: 2;
			width: 100%;
		}
		.triage-search {
			width: 100%;
		}
	}
	@container queue-header (min-width: 75rem) {
		.queue-headline,
		.queue-toolbar {
			--queue-search-width: clamp(24rem, 29cqi, 35rem);
			padding-inline-end: calc(var(--triage-gutter) + var(--queue-search-width) + 16px);
		}
		.queue-headline {
			display: flex;
			min-height: calc(max(2.5rem, var(--triage-control-height)) + 16px);
		}
		.queue-search-wrap {
			position: absolute;
			inset-block-start: 50%;
			inset-inline-end: var(--triage-gutter);
			width: var(--queue-search-width);
			margin-inline: 0;
			transform: translateY(-50%);
		}
	}
	@container queue-header (max-width: 51.99rem) {
		.queue-headline {
			gap: 6px 8px;
		}
		.queue-search-wrap {
			flex: 1 1 100%;
			width: 100%;
			margin-inline-start: 0;
		}
		.triage-search {
			width: 100%;
		}
	}
	@container queue-header (max-width: 38rem) {
		.queue-tab {
			gap: 4px;
			padding-inline: 8px;
		}
	}

	@media (max-width: 720px), (pointer: coarse) {
		.triage-search-kbd {
			display: none;
		}
		.triage-search-input {
			font-size: 1rem;
		}
	}

	@media (max-width: 959px) {
		.triage-split {
			display: block;
		}
		.triage-skeleton-col {
			border-inline-end: 0;
		}
		.triage-page:not(.triage-has-detail) :global(.triage-reading) {
			display: none;
		}
		.triage-has-detail :global(.triage-toolbar),
		.triage-has-detail :global(.triage-queue) {
			display: none;
		}
		.triage-has-detail .queue-headline {
			padding-block-end: 12px;
		}
	}

	.queue-tab:focus-visible,
	.queue-icon-btn:focus-visible,
	.triage-clear-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}

	@media (forced-colors: active) {
		.queue-tab,
		.queue-icon-btn,
		.triage-search {
			border: 1px solid ButtonText;
		}
		.queue-tab:is([aria-pressed='true'], [data-current-kind]),
		.queue-icon-btn[data-active] {
			border: 2px solid Highlight;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.queue-tab,
		.queue-icon-btn {
			transition: none;
		}
	}
</style>
