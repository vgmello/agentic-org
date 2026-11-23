<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { MediaQuery } from 'svelte/reactivity';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { navigating, page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import Flame from '@lucide/svelte/icons/flame';
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
	const scopeOpts = $derived(triageScopeOptions(allItems));
	const counts = $derived(triageTabCounts(allItems, scope));
	const filtered = $derived(
		filterTriageItems(allItems, {
			tab: activeTab,
			urgentOnly,
			scope,
			search: searchQuery
		})
	);
	const groups = $derived(groupTriageItems(filtered, activeTab, nowMs));
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
	const scopeCount = $derived((scope.team ? 1 : 0) + (scope.project ? 1 : 0));
	const hasActiveFilters = $derived(
		Boolean(searchQuery.trim() || urgentOnly || scope.team || scope.project)
	);
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

	function updateDenyDraft(itemId: string, value: string) {
		denyDrafts = { ...denyDrafts, [draftKey(organizationId, itemId)]: value };
	}

	function updateReplyDraft(itemId: string, value: string) {
		replyDrafts = { ...replyDrafts, [draftKey(organizationId, itemId)]: value };
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
			search: searchQuery
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

	function toggleUrgentOnly() {
		selectionVersion += 1;
		urgentOnly = !urgentOnly;
		decliningId = undefined;
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

	function filterByProject(projectId: string) {
		const nextProject = scope.project === projectId ? undefined : projectId;
		// Preserve the selected item on mobile when toggling its own project chip so the reading pane stays open.
		setScope({ ...scope, project: nextProject }, true);
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
	<div
		{@attach workspaceShortcuts}
		class="exec-card triage-workspace flex min-h-0 w-full flex-col overflow-hidden"
	>
		<!-- Header & Controls -->
		<header class="inbox-head triage-header flex flex-col gap-3 px-4 pt-4 pb-3 sm:px-5">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<div class="flex flex-wrap items-center gap-3">
					<h1
						id="shell-page-title"
						tabindex="-1"
						class="triage-h1 text-xl font-semibold tracking-tight focus:outline-none"
					>
						Inbox
					</h1>
					<span
						role="status"
						class="triage-waiting-pill inline-flex items-center gap-2 px-2.5 py-1 text-xs font-medium"
					>
						<span
							class={[
								'triage-waiting-dot size-2 shrink-0 rounded-full',
								totalOpenCount === 0 && 'is-ok'
							]}
							aria-hidden="true"
						></span>
						<span class="tabular-nums">
							{totalOpenCount === 0
								? '0 waiting on you — all caught up'
								: `${totalOpenCount} waiting on you`}
						</span>
					</span>
				</div>

				<!-- Search + Urgency Toggle + Show Team/Project Filter -->
				<div class="triage-controls flex min-w-0 flex-wrap items-center gap-2">
					<div class="triage-search flex items-center gap-2 px-2.5">
						<Search class="triage-search-ico size-3.5 shrink-0" aria-hidden="true" />
						<label class="sr-only" for="triage-search-input">Search inbox</label>
						<input
							bind:this={searchInputEl}
							value={searchQuery}
							oninput={(e) => setSearchQuery(e.currentTarget.value)}
							id="triage-search-input"
							type="text"
							autocomplete="off"
							placeholder={shortcutsEnabled ? 'Search inbox… (/)' : 'Search inbox…'}
							class="triage-search-input min-h-9 min-w-0 flex-1 self-stretch bg-transparent text-xs outline-none max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
						/>
						{#if searchQuery}
							<button
								type="button"
								onclick={() => {
									setSearchQuery('');
									searchInputEl?.focus();
								}}
								aria-label="Clear search"
								class="triage-clear-btn inline-flex min-h-9 min-w-9 items-center justify-center max-sm:min-h-11 max-sm:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
							>
								<X class="size-3.5" aria-hidden="true" />
							</button>
						{/if}
					</div>

					<button
						type="button"
						aria-pressed={urgentOnly}
						onclick={toggleUrgentOnly}
						class={[
							'triage-ctrl-btn inline-flex min-h-9 items-center gap-1.5 px-3 text-xs font-semibold max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11',
							urgentOnly && 'is-active'
						]}
					>
						<Flame class="size-3.5 shrink-0" aria-hidden="true" />
						<span>{urgentOnly ? 'Urgent only' : 'All'}</span>
					</button>

					<DropdownMenu.Root>
						<DropdownMenu.Trigger>
							{#snippet child({ props })}
								<button
									{...props}
									type="button"
									aria-label={scopeCount > 0
										? `Show filters, ${scopeCount} active`
										: 'Show team or project filter'}
									class={[
										'triage-ctrl-btn inline-flex min-h-9 items-center gap-1.5 px-3 text-xs font-semibold max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11',
										scopeCount > 0 && 'is-active'
									]}
								>
									<SlidersHorizontal class="size-3.5 shrink-0" aria-hidden="true" />
									<span>Show</span>
									{#if scopeCount > 0}
										<span
											class="triage-count-badge px-1.5 py-0.5 font-mono text-[10px] tabular-nums"
										>
											{scopeCount}
										</span>
									{/if}
									<ChevronDown class="size-3.5 shrink-0 opacity-75" aria-hidden="true" />
								</button>
							{/snippet}
						</DropdownMenu.Trigger>
						<DropdownMenu.Content
							preventScroll={false}
							align="end"
							class="w-60 max-w-[calc(100vw-2rem)] motion-reduce:animate-none! motion-reduce:transition-none! motion-reduce:data-[state=closed]:animate-none! motion-reduce:data-[state=open]:animate-none!"
						>
							<DropdownMenu.Label>Team</DropdownMenu.Label>
							<DropdownMenu.RadioGroup
								value={scope.team ?? ''}
								onValueChange={(val) => setScope({ ...scope, team: val || undefined })}
							>
								<DropdownMenu.RadioItem
									value=""
									class="min-h-9 max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
								>
									All teams
								</DropdownMenu.RadioItem>
								{#each scopeOpts.teams as team (team.id)}
									<DropdownMenu.RadioItem
										value={team.id}
										class="min-h-9 max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
									>
										{team.name}
									</DropdownMenu.RadioItem>
								{/each}
							</DropdownMenu.RadioGroup>
							<DropdownMenu.Separator />
							<DropdownMenu.Label>Project</DropdownMenu.Label>
							<DropdownMenu.RadioGroup
								value={scope.project ?? ''}
								onValueChange={(val) => setScope({ ...scope, project: val || undefined })}
							>
								<DropdownMenu.RadioItem
									value=""
									class="min-h-9 max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
								>
									All projects
								</DropdownMenu.RadioItem>
								{#each scopeOpts.projects as proj (proj.id)}
									<DropdownMenu.RadioItem
										value={proj.id}
										class="min-h-9 max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
									>
										{proj.name}
									</DropdownMenu.RadioItem>
								{/each}
							</DropdownMenu.RadioGroup>
							{#if scopeCount > 0}
								<DropdownMenu.Separator />
								<DropdownMenu.Group>
									<DropdownMenu.Item
										onSelect={() => setScope({})}
										class="min-h-9 max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
									>
										Clear team &amp; project filters
									</DropdownMenu.Item>
								</DropdownMenu.Group>
							{/if}
						</DropdownMenu.Content>
					</DropdownMenu.Root>
				</div>
			</div>

			<!-- 5 Primary Category Filters + "+ More" Overflow Menu -->
			<div class="flex flex-wrap items-center gap-1.5">
				<div role="group" aria-label="Inbox categories" class="flex flex-wrap items-center gap-1.5">
					{#each PRIMARY_TABS as tab (tab.id)}
						{@const isSelected = activeTab === tab.id}
						<button
							type="button"
							aria-pressed={isSelected}
							onclick={() => selectTab(tab.id)}
							class={[
								'triage-tab inline-flex min-h-9 items-center gap-1.5 px-3 text-xs font-semibold max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11',
								isSelected && 'is-on'
							]}
						>
							<span>{tab.label}</span>
							<span class="triage-tab-count font-mono text-[11px] tabular-nums">
								{counts[tab.id]}
							</span>
						</button>
					{/each}
				</div>

				<DropdownMenu.Root>
					<DropdownMenu.Trigger>
						{#snippet child({ props })}
							<button
								{...props}
								type="button"
								class={[
									'triage-tab inline-flex min-h-9 items-center gap-1.5 px-3 text-xs font-semibold max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11',
									overflowActive && 'is-on'
								]}
							>
								<span>{overflowActive ? overflowActiveLabel : '+ More'}</span>
								{#if overflowActive}
									<span class="triage-tab-count font-mono text-[11px] tabular-nums">
										{counts[activeTab]}
									</span>
								{/if}
								<ChevronDown class="size-3.5 shrink-0 opacity-75" aria-hidden="true" />
							</button>
						{/snippet}
					</DropdownMenu.Trigger>
					<DropdownMenu.Content
						preventScroll={false}
						align="start"
						class="w-48 motion-reduce:animate-none! motion-reduce:transition-none! motion-reduce:data-[state=closed]:animate-none! motion-reduce:data-[state=open]:animate-none!"
					>
						<DropdownMenu.Group>
							{#each OVERFLOW_TABS as tab (tab.id)}
								<DropdownMenu.Item
									onSelect={() => selectTab(tab.id)}
									class="flex min-h-9 items-center justify-between gap-2 max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
								>
									<span>{tab.label}</span>
									<span class="triage-menu-count font-mono text-xs tabular-nums">
										{counts[tab.id]}
									</span>
								</DropdownMenu.Item>
							{/each}
						</DropdownMenu.Group>
					</DropdownMenu.Content>
				</DropdownMenu.Root>
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
					selectedId={effectiveSelectedId}
					{busyId}
					{searchQuery}
					{hasActiveFilters}
					onSelect={selectItem}
					onResetFilters={resetFilters}
				/>
				<InboxDetailPane
					item={selectedItem}
					{searchQuery}
					{busyId}
					{actionLocked}
					{shortcutsEnabled}
					{decliningId}
					declineReason={activeDeclineReason}
					replyText={activeReplyText}
					onDeclineReasonChange={updateDenyDraft}
					onReplyTextChange={updateReplyDraft}
					onBackMobile={closeMobileDetail}
					onFilterProject={filterByProject}
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
	</div>
</div>

<style>
	.triage-workspace {
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
	.triage-header {
		border-bottom: 1px solid var(--exec-border);
	}
	.triage-h1 {
		color: var(--exec-ink);
	}
	.triage-waiting-pill {
		border-radius: var(--exec-pill-radius);
		background: var(--exec-well);
		color: var(--exec-body);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.triage-waiting-dot {
		background: var(--tone-accent);
	}
	.triage-waiting-dot.is-ok {
		background: var(--tone-ok);
	}
	.triage-search {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		width: 210px;
		max-width: 100%;
	}
	.triage-search:focus-within {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}
	.triage-search-ico {
		color: var(--exec-muted);
	}
	.triage-search-input {
		color: var(--exec-ink);
	}
	.triage-search-input::placeholder {
		color: var(--exec-muted);
		opacity: 1;
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
	.triage-ctrl-btn {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		cursor: pointer;
		transition: background-color 120ms ease-out;
	}
	.triage-ctrl-btn:hover {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring-hover);
	}
	.triage-ctrl-btn.is-active {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.triage-count-badge {
		border-radius: var(--exec-pill-radius);
		background: var(--tone-accent-bg);
		color: var(--exec-accent-ink);
	}
	.triage-tab {
		border-radius: var(--exec-ctrl-radius);
		color: var(--exec-muted);
		background: transparent;
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.triage-tab:hover {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.triage-tab.is-on {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.triage-tab-count,
	.triage-menu-count {
		color: var(--exec-muted);
	}
	.triage-tab:hover .triage-tab-count {
		color: var(--exec-ink);
	}
	.triage-tab.is-on .triage-tab-count {
		color: var(--exec-accent-ink);
	}
	.triage-split {
		display: grid;
		grid-template-columns: minmax(320px, 365px) minmax(0, 1fr);
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
		.triage-has-detail :global(.triage-header),
		.triage-has-detail :global(.triage-queue) {
			display: none;
		}
	}

	@media (max-width: 520px) {
		.triage-controls,
		.triage-search {
			width: 100%;
		}
	}

	.triage-ctrl-btn:focus-visible,
	.triage-tab:focus-visible,
	.triage-clear-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}

	@media (forced-colors: active) {
		.triage-tab,
		.triage-ctrl-btn,
		.triage-search {
			border: 1px solid ButtonText;
		}
		.triage-tab.is-on,
		.triage-tab[aria-pressed='true'],
		.triage-ctrl-btn.is-active,
		.triage-ctrl-btn[aria-pressed='true'] {
			border: 2px solid Highlight;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.triage-ctrl-btn,
		.triage-tab {
			transition: none;
		}
	}
</style>
