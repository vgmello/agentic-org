<script lang="ts">
	import { onDestroy } from 'svelte';
	import { resolve } from '$app/paths';
	import { Tooltip } from 'bits-ui';
	import Activity from '@lucide/svelte/icons/activity';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Flag from '@lucide/svelte/icons/flag';
	import Layers from '@lucide/svelte/icons/layers';
	import { activityApi } from '$lib/modules/activity/api';
	import type { ActivityQuery, ActivityRow } from '$lib/modules/activity/types';
	import { inboxApi } from '$lib/modules/inbox/api';
	import type { InboxItem } from '$lib/modules/inbox/types';
	import { accountZone } from '$lib/modules/preferences';
	import { useShellData } from '$lib/platform/shell/shell-data.svelte';
	import type { OrgSnapshot } from '$lib/proto';
	import ActivityOpsRail from './ActivityOpsRail.svelte';
	import ActivityStreamPanel from './ActivityStreamPanel.svelte';
	import AssemblyLineMatrix from './AssemblyLineMatrix.svelte';
	import {
		deriveActivityCommandCenter,
		deriveDecisionStreamEntries,
		mergeLiveActivityRows,
		type FeedPhase,
		type StreamFilterTab
	} from './assembly-model';

	const FEED_LIMIT = 200;

	let {
		organizationId,
		organizationName,
		snapshot
	}: {
		organizationId: string;
		organizationName: string;
		snapshot: OrgSnapshot;
	} = $props();

	const shellData = useShellData();

	// CRITICAL DEFAULT: Assembly Line Matrix is shown FIRST by default.
	let viewMode = $state<'assembly' | 'feed'>('assembly');
	let panelOpen = $state(typeof window !== 'undefined' ? window.innerWidth >= 1024 : true);
	let toolsBtnEl = $state<HTMLButtonElement | null>(null);
	let projectScope = $state<string>('all');
	let streamFilter = $state<StreamFilterTab>('all');
	let activityRows = $state.raw<ActivityRow[] | undefined>(undefined);
	let feedPhase = $state<FeedPhase>('loading');
	let feedError = $state<string | undefined>(undefined);
	let feedMore = $state(0);
	let feedRetrySeq = $state(0);
	let decisionRows = $state.raw<ActivityRow[] | undefined>(undefined);
	let decisionPhase = $state<FeedPhase>('loading');
	let decisionError = $state<string | undefined>(undefined);
	let decisionMore = $state(0);
	let decisionRetrySeq = $state(0);
	let directInboxItems = $state.raw<InboxItem[] | undefined>(undefined);
	let inboxPhase = $state<FeedPhase>('loading');
	let inboxSubRetrySeq = $state(0);
	let activitySubRetrySeq = $state(0);
	let inboxSubFailedOrg = $state<string | undefined>(undefined);
	let activitySubFailedOrg = $state<string | undefined>(undefined);
	let inboxCatchupFailedOrg = $state<string | undefined>(undefined);
	let activityCatchupFailedOrg = $state<string | undefined>(undefined);
	let decisionCatchupFailedOrg = $state<string | undefined>(undefined);
	let reconnectingOrg = $state<string | undefined>(undefined);
	let headerRetryActive = $state(false);
	let shellRefreshing = $state(false);
	let nowMs = $state(Date.now());

	let bufferedAllRows: ActivityRow[] = [];
	let bufferedDecisionRows: ActivityRow[] = [];
	let inboxInFlight = false;
	let pendingInboxReconcile = false;
	let mainInFlight = false;
	let pendingMainReconcile = false;
	let decisionInFlight = false;
	let pendingDecisionReconcile = false;
	let reconnectPendingInboxSub = false;
	let reconnectPendingActivitySub = false;
	let reconnectPendingInboxFetch = false;
	let reconnectPendingMainFetch = false;
	let reconnectPendingDecisionFetch = false;
	let shellRefreshSeq = 0;
	let destroyed = false;

	const inboxSubFailed = $derived(inboxSubFailedOrg === organizationId);
	const activitySubFailed = $derived(activitySubFailedOrg === organizationId);
	const catchupFailed = $derived(
		inboxCatchupFailedOrg === organizationId ||
			activityCatchupFailedOrg === organizationId ||
			(viewMode === 'feed' &&
				streamFilter === 'decision' &&
				decisionCatchupFailedOrg === organizationId)
	);
	const isReconnecting = $derived(reconnectingOrg === organizationId);
	const showLiveBanner = $derived(
		inboxSubFailed || activitySubFailed || catchupFailed || isReconnecting
	);

	function restoreFocusIfBannerDismissing(id: string) {
		if (destroyed || typeof document === 'undefined' || id !== organizationId) return;
		const stillVisible =
			reconnectingOrg === id ||
			inboxSubFailedOrg === id ||
			activitySubFailedOrg === id ||
			inboxCatchupFailedOrg === id ||
			activityCatchupFailedOrg === id ||
			(viewMode === 'feed' && streamFilter === 'decision' && decisionCatchupFailedOrg === id);
		if (stillVisible) return;
		const active = document.activeElement;
		if (active instanceof HTMLElement && active.closest('.cmd-live-banner')) {
			active.closest('.cmd-page')?.querySelector<HTMLElement>('.cmd-status-slot')?.focus();
		}
	}

	function maybeFinishReconnect(id: string) {
		if (destroyed || reconnectingOrg !== id) return;
		if (
			reconnectPendingInboxSub ||
			reconnectPendingActivitySub ||
			reconnectPendingInboxFetch ||
			reconnectPendingMainFetch ||
			reconnectPendingDecisionFetch
		) {
			return;
		}
		reconnectingOrg = undefined;
		restoreFocusIfBannerDismissing(id);
	}

	onDestroy(() => {
		destroyed = true;
	});

	const shellReady = $derived(
		shellData.loaded && shellData.loadedOrganizationId === organizationId
	);

	const workUnavailableReason = $derived.by<string | undefined>(() => {
		if (!shellReady || !shellData.workError) return undefined;
		const err = shellData.workError;
		if (err instanceof Error && err.message) return err.message;
		if (
			typeof err === 'object' &&
			err !== null &&
			'message' in err &&
			typeof (err as { message: unknown }).message === 'string'
		) {
			return (err as { message: string }).message;
		}
		return 'Could not load work items from the office.';
	});

	const workPhase = $derived<FeedPhase>(
		!shellReady ? 'loading' : workUnavailableReason ? 'error' : 'ready'
	);

	const liveSnapshot = $derived(shellReady && shellData.snapshot ? shellData.snapshot : snapshot);

	const workItems = $derived(workPhase === 'ready' ? shellData.workItems : []);
	const inboxItems = $derived(inboxPhase === 'ready' ? (directInboxItems ?? []) : []);

	const activeProjectIds = $derived(
		new Set((liveSnapshot.projects ?? []).filter((p) => !p.archived).map((p) => p.id))
	);

	const effectiveProjectScope = $derived(
		projectScope === 'all' || activeProjectIds.has(projectScope) ? projectScope : 'all'
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

	// Race-guarded direct Inbox fetch + live subscription so approvals stay current.
	let inboxLoadSeq = 0;

	function fetchInbox(id: string, showLoading: boolean) {
		const seq = ++inboxLoadSeq;
		inboxInFlight = true;
		if (showLoading) {
			pendingInboxReconcile = false;
			inboxPhase = 'loading';
			directInboxItems = undefined;
		}
		void inboxApi
			.list(id)
			.then((items) => {
				if (seq !== inboxLoadSeq || id !== organizationId || destroyed) return;
				directInboxItems = items;
				inboxPhase = 'ready';
				if (inboxCatchupFailedOrg === id) {
					inboxCatchupFailedOrg = undefined;
					restoreFocusIfBannerDismissing(id);
				}
			})
			.catch(() => {
				if (seq !== inboxLoadSeq || id !== organizationId || destroyed) return;
				if (!showLoading && pendingInboxReconcile) return;
				directInboxItems = undefined;
				inboxPhase = 'error';
				if (!showLoading) inboxCatchupFailedOrg = id;
			})
			.finally(() => {
				if (seq !== inboxLoadSeq || id !== organizationId || destroyed) return;
				inboxInFlight = false;
				if (pendingInboxReconcile) {
					pendingInboxReconcile = false;
					fetchInbox(id, false);
				} else {
					reconnectPendingInboxFetch = false;
					maybeFinishReconnect(id);
					if (!shellRefreshing) {
						headerRetryActive = false;
					}
				}
			});
	}

	function scheduleInboxReconcile(id: string) {
		if (inboxInFlight) {
			pendingInboxReconcile = true;
		} else {
			fetchInbox(id, false);
		}
	}

	$effect(() => {
		const id = organizationId;
		void accountZone.ensure(id);
		fetchInbox(id, true);
		return () => {
			inboxLoadSeq += 1;
			shellRefreshSeq += 1;
			inboxInFlight = false;
			pendingInboxReconcile = false;
			reconnectPendingInboxSub = false;
			reconnectPendingActivitySub = false;
			reconnectPendingInboxFetch = false;
			reconnectPendingMainFetch = false;
			reconnectPendingDecisionFetch = false;
			headerRetryActive = false;
			shellRefreshing = false;
		};
	});

	$effect(() => {
		const id = organizationId;
		const subRetry = inboxSubRetrySeq;
		void subRetry;
		let cancelled = false;
		let unsub: (() => void) | undefined;
		void inboxApi
			.subscribe(id, () => {
				if (!cancelled) scheduleInboxReconcile(id);
			})
			.then((stop) => {
				if (cancelled) {
					stop();
					return;
				}
				if (inboxSubFailedOrg === id) {
					inboxSubFailedOrg = undefined;
					restoreFocusIfBannerDismissing(id);
				}
				unsub = stop;
				if (reconnectingOrg === id) {
					reconnectPendingInboxFetch = true;
				}
				reconnectPendingInboxSub = false;
				// Reconcile once without preempting an in-flight initial Inbox load.
				scheduleInboxReconcile(id);
				maybeFinishReconnect(id);
			})
			.catch(() => {
				if (cancelled || destroyed) return;
				inboxSubFailedOrg = id;
				reconnectPendingInboxSub = false;
				maybeFinishReconnect(id);
			});

		return () => {
			cancelled = true;
			unsub?.();
		};
	});

	// Race-guarded API-level project-scoped `kind: 'all'` feed load (feeds stream, assembly, and 7d throughput).
	let loadSeq = 0;

	function fetchMainFeed(
		id: string,
		query: ActivityQuery,
		showLoading: boolean,
		preserveBuffer = false
	) {
		const seq = ++loadSeq;
		mainInFlight = true;
		if (!preserveBuffer) {
			bufferedAllRows = [];
		}
		if (showLoading) {
			pendingMainReconcile = false;
			feedPhase = 'loading';
			feedError = undefined;
			feedMore = 0;
			activityRows = undefined;
		}

		void activityApi
			.feed(id, query, FEED_LIMIT)
			.then((res) => {
				if (seq !== loadSeq || id !== organizationId || destroyed) return;
				// Treat the fresh server snapshot as the authoritative baseline and replay only live arrivals buffered during this request.
				const merged = mergeLiveActivityRows(
					res.rows,
					bufferedAllRows,
					query,
					FEED_LIMIT,
					res.more
				);
				bufferedAllRows = [];
				activityRows = merged.rows;
				feedMore = merged.more;
				feedPhase = 'ready';
				feedError = undefined;
				if (activityCatchupFailedOrg === id) {
					activityCatchupFailedOrg = undefined;
					restoreFocusIfBannerDismissing(id);
				}
				nowMs = Date.now();
				// Close buffering synchronously in the same turn as draining so live arrivals cannot be lost.
				mainInFlight = false;
				if (pendingMainReconcile) {
					pendingMainReconcile = false;
					fetchMainFeed(id, activeAllQuery, false);
				} else {
					reconnectPendingMainFetch = false;
					maybeFinishReconnect(id);
				}
			})
			.catch((err: unknown) => {
				if (seq !== loadSeq || id !== organizationId || destroyed) return;
				if (!showLoading && feedPhase === 'ready') {
					const merged = mergeLiveActivityRows(
						activityRows ?? [],
						bufferedAllRows,
						query,
						FEED_LIMIT,
						feedMore
					);
					bufferedAllRows = [];
					if (merged.changed) {
						activityRows = merged.rows;
						feedMore = merged.more;
						nowMs = Date.now();
					}
					mainInFlight = false;
					if (pendingMainReconcile) {
						pendingMainReconcile = false;
						fetchMainFeed(id, activeAllQuery, false);
					} else {
						activityCatchupFailedOrg = id;
						reconnectPendingMainFetch = false;
						maybeFinishReconnect(id);
					}
					return;
				}
				if (pendingMainReconcile) {
					// Carry buffered live arrivals forward into the queued same-scope recovery fetch.
					pendingMainReconcile = false;
					fetchMainFeed(id, activeAllQuery, false, true);
					return;
				}
				bufferedAllRows = [];
				mainInFlight = false;
				activityRows = undefined;
				feedMore = 0;
				feedPhase = 'error';
				feedError =
					err instanceof Error && err.message ? err.message : 'Could not load the activity feed.';
				if (!showLoading) activityCatchupFailedOrg = id;
				nowMs = Date.now();
				reconnectPendingMainFetch = false;
				maybeFinishReconnect(id);
			});
	}

	$effect(() => {
		const id = organizationId;
		const query = activeAllQuery;
		const retryToken = feedRetrySeq;
		void retryToken;

		fetchMainFeed(id, query, true);
		return () => {
			loadSeq += 1;
			mainInFlight = false;
			pendingMainReconcile = false;
			bufferedAllRows = [];
		};
	});

	// Dedicated `kind: 'decision'` fetch when Decisions tab is active (matchesQuery excludes decisions from `all`).
	let decisionLoadSeq = 0;

	function fetchDecisionFeed(
		id: string,
		query: ActivityQuery,
		showLoading: boolean,
		preserveBuffer = false
	) {
		const seq = ++decisionLoadSeq;
		decisionInFlight = true;
		if (!preserveBuffer) {
			bufferedDecisionRows = [];
		}
		if (showLoading) {
			pendingDecisionReconcile = false;
			decisionPhase = 'loading';
			decisionError = undefined;
			decisionMore = 0;
			decisionRows = undefined;
		}

		void activityApi
			.feed(id, query, FEED_LIMIT)
			.then((res) => {
				if (seq !== decisionLoadSeq || id !== organizationId || destroyed) return;
				// Authoritative server snapshot baseline + live arrivals buffered during this request.
				const merged = mergeLiveActivityRows(
					res.rows,
					bufferedDecisionRows,
					query,
					FEED_LIMIT,
					res.more
				);
				bufferedDecisionRows = [];
				decisionRows = merged.rows;
				decisionMore = merged.more;
				decisionPhase = 'ready';
				decisionError = undefined;
				if (decisionCatchupFailedOrg === id) {
					decisionCatchupFailedOrg = undefined;
					restoreFocusIfBannerDismissing(id);
				}
				nowMs = Date.now();
				// Close buffering synchronously in the same turn as draining so live arrivals cannot be lost.
				decisionInFlight = false;
				if (pendingDecisionReconcile && viewMode === 'feed' && streamFilter === 'decision') {
					pendingDecisionReconcile = false;
					fetchDecisionFeed(id, activeDecisionQuery, false);
				} else {
					pendingDecisionReconcile = false;
					reconnectPendingDecisionFetch = false;
					maybeFinishReconnect(id);
				}
			})
			.catch((err: unknown) => {
				if (seq !== decisionLoadSeq || id !== organizationId || destroyed) return;
				if (!showLoading && decisionPhase === 'ready') {
					const merged = mergeLiveActivityRows(
						decisionRows ?? [],
						bufferedDecisionRows,
						query,
						FEED_LIMIT,
						decisionMore
					);
					bufferedDecisionRows = [];
					if (merged.changed) {
						decisionRows = merged.rows;
						decisionMore = merged.more;
						nowMs = Date.now();
					}
					decisionInFlight = false;
					if (pendingDecisionReconcile && viewMode === 'feed' && streamFilter === 'decision') {
						pendingDecisionReconcile = false;
						fetchDecisionFeed(id, activeDecisionQuery, false);
					} else {
						pendingDecisionReconcile = false;
						decisionCatchupFailedOrg = id;
						reconnectPendingDecisionFetch = false;
						maybeFinishReconnect(id);
					}
					return;
				}
				if (pendingDecisionReconcile && viewMode === 'feed' && streamFilter === 'decision') {
					// Carry buffered live arrivals forward into the queued same-scope recovery fetch.
					pendingDecisionReconcile = false;
					fetchDecisionFeed(id, activeDecisionQuery, false, true);
					return;
				}
				pendingDecisionReconcile = false;
				bufferedDecisionRows = [];
				decisionInFlight = false;
				decisionRows = undefined;
				decisionMore = 0;
				decisionPhase = 'error';
				decisionError =
					err instanceof Error && err.message ? err.message : 'Could not load decisions.';
				if (!showLoading) decisionCatchupFailedOrg = id;
				nowMs = Date.now();
				reconnectPendingDecisionFetch = false;
				maybeFinishReconnect(id);
			});
	}

	$effect(() => {
		const id = organizationId;
		const query = activeDecisionQuery;
		const active = viewMode === 'feed' && streamFilter === 'decision';
		const retryToken = decisionRetrySeq;
		void retryToken;
		if (!active) return;

		fetchDecisionFeed(id, query, true);
		return () => {
			decisionLoadSeq += 1;
			decisionInFlight = false;
			pendingDecisionReconcile = false;
			bufferedDecisionRows = [];
			if (reconnectPendingDecisionFetch) {
				reconnectPendingDecisionFetch = false;
				maybeFinishReconnect(id);
			}
		};
	});

	// Live activity subscription buffers arrivals during any in-flight load/reconcile, upserts by ID,
	// filters via matchesQuery, tracks evictions, and reconciles cleanly once registered without preempting initial loads.
	$effect(() => {
		const id = organizationId;
		const subRetry = activitySubRetrySeq;
		void subRetry;
		let cancelled = false;
		let unsub: (() => void) | undefined;

		void activityApi
			.subscribe(id, (row) => {
				if (cancelled) return;

				if (mainInFlight) {
					bufferedAllRows.push(row);
				} else if (feedPhase === 'ready') {
					const merged = mergeLiveActivityRows(
						activityRows ?? [],
						[row],
						activeAllQuery,
						FEED_LIMIT,
						feedMore
					);
					if (merged.changed) {
						activityRows = merged.rows;
						feedMore = merged.more;
						nowMs = Date.now();
					}
				}

				if (viewMode === 'feed' && streamFilter === 'decision') {
					if (decisionInFlight) {
						bufferedDecisionRows.push(row);
					} else if (decisionPhase === 'ready') {
						const mergedDec = mergeLiveActivityRows(
							decisionRows ?? [],
							[row],
							activeDecisionQuery,
							FEED_LIMIT,
							decisionMore
						);
						if (mergedDec.changed) {
							decisionRows = mergedDec.rows;
							decisionMore = mergedDec.more;
							nowMs = Date.now();
						}
					}
				}
			})
			.then((stop) => {
				if (cancelled) {
					stop();
					return;
				}
				if (activitySubFailedOrg === id) {
					activitySubFailedOrg = undefined;
					restoreFocusIfBannerDismissing(id);
				}
				unsub = stop;
				const trackingReconnect = reconnectingOrg === id;
				// Reconcile after initial load settles (never preempt an in-flight initial snapshot; also recovers from transient initial error).
				if (mainInFlight) {
					if (trackingReconnect) reconnectPendingMainFetch = true;
					pendingMainReconcile = true;
				} else if (feedPhase === 'ready' || feedPhase === 'error') {
					if (trackingReconnect) reconnectPendingMainFetch = true;
					fetchMainFeed(id, activeAllQuery, false);
				}
				if (viewMode === 'feed' && streamFilter === 'decision') {
					if (decisionInFlight) {
						if (trackingReconnect) reconnectPendingDecisionFetch = true;
						pendingDecisionReconcile = true;
					} else if (decisionPhase === 'ready' || decisionPhase === 'error') {
						if (trackingReconnect) reconnectPendingDecisionFetch = true;
						fetchDecisionFeed(id, activeDecisionQuery, false);
					}
				}
				reconnectPendingActivitySub = false;
				maybeFinishReconnect(id);
			})
			.catch(() => {
				if (cancelled || destroyed) return;
				activitySubFailedOrg = id;
				reconnectPendingActivitySub = false;
				maybeFinishReconnect(id);
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
			activityRows: activityRows ?? [],
			projectScope: effectiveProjectScope,
			ownerId: shellData.ownerId,
			feedPhase,
			feedMore,
			workPhase,
			inboxPhase,
			nowMs
		})
	);

	const decisionEntries = $derived(
		deriveDecisionStreamEntries({
			snapshot: liveSnapshot,
			decisionRows: decisionRows ?? [],
			workItems,
			inboxItems,
			projectScope: effectiveProjectScope,
			ownerId: shellData.ownerId,
			decisionPhase,
			workPhase,
			inboxPhase
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
		headerRetryActive && (shellRefreshing || !shellReady || inboxPhase === 'loading')
	);

	function retryFeed() {
		if (activitySubFailed) {
			activitySubRetrySeq += 1;
		}
		feedRetrySeq += 1;
	}

	function retryDecisions() {
		if (activitySubFailed) {
			activitySubRetrySeq += 1;
		}
		decisionRetrySeq += 1;
	}

	function reconnectLiveUpdates(event: MouseEvent) {
		if (isReconnecting) return;
		const id = organizationId;
		reconnectingOrg = id;
		// Keep a visible focus indicator on the persistent banner while the action button becomes busy/disabled.
		(event.currentTarget as HTMLElement | null)?.closest<HTMLElement>('.cmd-live-banner')?.focus();

		if (inboxSubFailed) {
			reconnectPendingInboxSub = true;
			inboxSubRetrySeq += 1;
		} else if (inboxCatchupFailedOrg === id) {
			reconnectPendingInboxFetch = true;
			scheduleInboxReconcile(id);
		}

		if (activitySubFailed) {
			reconnectPendingActivitySub = true;
			activitySubRetrySeq += 1;
		} else {
			if (activityCatchupFailedOrg === id) {
				reconnectPendingMainFetch = true;
				if (mainInFlight) {
					pendingMainReconcile = true;
				} else {
					fetchMainFeed(id, activeAllQuery, false);
				}
			}
			if (decisionCatchupFailedOrg === id && viewMode === 'feed' && streamFilter === 'decision') {
				reconnectPendingDecisionFetch = true;
				if (decisionInFlight) {
					pendingDecisionReconcile = true;
				} else {
					fetchDecisionFeed(id, activeDecisionQuery, false);
				}
			}
		}

		maybeFinishReconnect(id);
	}

	function retryShellAndInbox() {
		const id = organizationId;
		if (inboxSubFailed) {
			inboxSubRetrySeq += 1;
		}
		if (activitySubFailed) {
			activitySubRetrySeq += 1;
		}
		fetchInbox(id, true);
		const refreshSeq = ++shellRefreshSeq;
		shellRefreshing = true;
		void shellData.refresh().finally(() => {
			if (destroyed || id !== organizationId || refreshSeq !== shellRefreshSeq) return;
			shellRefreshing = false;
			if (!inboxInFlight) headerRetryActive = false;
		});
	}

	function handleHeaderRetry(event: MouseEvent) {
		if (isHeaderRetrying) return;
		headerRetryActive = true;
		const slot = (event.currentTarget as HTMLElement | null)?.closest<HTMLElement>(
			'.cmd-status-slot'
		);
		// Keep a visible focus indicator on the persistent status wrapper while the Retry button becomes busy/disabled.
		slot?.focus();
		retryShellAndInbox();
	}

	const streamTabs: { id: StreamFilterTab; label: string }[] = [
		{ id: 'all', label: 'All' },
		{ id: 'needs_you', label: 'Needs you' },
		{ id: 'work', label: 'Work' },
		{ id: 'run', label: 'Runs' },
		{ id: 'decision', label: 'Decisions' },
		{ id: 'conversation', label: 'Chat' },
		{ id: 'org', label: 'Org' }
	];

	function handleStreamTabKeydown(event: KeyboardEvent, index: number) {
		let nextIndex: number;
		if (event.key === 'ArrowRight') {
			nextIndex = (index + 1) % streamTabs.length;
		} else if (event.key === 'ArrowLeft') {
			nextIndex = (index - 1 + streamTabs.length) % streamTabs.length;
		} else if (event.key === 'Home') {
			nextIndex = 0;
		} else if (event.key === 'End') {
			nextIndex = streamTabs.length - 1;
		} else {
			return;
		}

		event.preventDefault();
		const targetTab = streamTabs[nextIndex];
		streamFilter = targetTab.id;
		const tablist = (event.currentTarget as HTMLElement | null)?.closest('[role="tablist"]');
		tablist?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[nextIndex]?.focus();
	}

	function handlePanelKeydown(event: KeyboardEvent) {
		if (event.defaultPrevented || event.isComposing || event.key !== 'Escape' || !panelOpen) return;
		event.preventDefault();
		panelOpen = false;
		toolsBtnEl?.focus();
	}
</script>

<div class="act-page cmd-page activity-workspace">
	{#if showLiveBanner}
		<div class="cmd-live-banner" role="status" tabindex="-1">
			<span class="cmd-live-text">
				<AlertTriangle class="size-3.5 shrink-0" aria-hidden="true" />
				<span>
					{#if isReconnecting}
						Reconnecting live updates…
					{:else if inboxSubFailed || activitySubFailed}
						Live updates paused
					{:else}
						Live catch-up failed — snapshots may be stale
					{/if}
				</span>
			</span>
			<button
				type="button"
				disabled={isReconnecting}
				aria-busy={isReconnecting}
				onclick={reconnectLiveUpdates}
				class="cmd-live-reconnect min-h-9 min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
			>
				{#if isReconnecting}
					Reconnecting…
				{:else if inboxSubFailed || activitySubFailed}
					Reconnect
				{:else}
					Retry
				{/if}
			</button>
		</div>
	{/if}

	<!-- Command Center Section Header & Aligned Toolbar (poc/operations.css 19c88bd5) -->
	<div class="section-header activity-header">
		<header class="ph cmd-header">
			<div class="ph-t cmd-title-group">
				<h1 id="shell-page-title" tabindex="-1" class="cmd-h1 focus:outline-none">Activity</h1>
				<p class="cmd-subtitle">{headerSubtitle}</p>
			</div>

			<!-- Waiting-on-you Pill (honest loading/unavailable states with visible Retry focus) -->
			<div class="cmd-status-slot" tabindex="-1">
				{#if workUnavailableReason || inboxPhase === 'error' || isHeaderRetrying}
					<button
						type="button"
						disabled={isHeaderRetrying}
						aria-busy={isHeaderRetrying}
						onclick={handleHeaderRetry}
						class="cmd-needs-pill is-warn min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
					>
						<AlertTriangle class="size-3.5" aria-hidden="true" />
						<span>
							{#if isHeaderRetrying}
								Retrying…
							{:else if workUnavailableReason}
								Retry work load
							{:else}
								Inbox unavailable
							{/if}
						</span>
					</button>
				{:else if !shellReady || inboxPhase === 'loading'}
					<span
						class="cmd-needs-pill is-neutral min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
					>
						<span>Loading status…</span>
					</span>
				{:else if model.needsYouCount > 0}
					<a
						href={resolve('/inbox')}
						class="cmd-needs-pill is-warn min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
					>
						<span class="cmd-needs-dot" aria-hidden="true"></span>
						<span class="tabular-nums">{model.needsYouCount} need you</span>
					</a>
				{:else}
					<span
						class="cmd-needs-pill is-ok min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
					>
						<CheckCircle2 class="size-3.5" aria-hidden="true" />
						<span>All caught up</span>
					</span>
				{/if}
			</div>
		</header>

		<div class="section-toolbar activity-toolbar">
			<!-- Segmented View Toggle: Assembly line (FIRST, active by default) | Activity (SECOND) -->
			<div
				class="exec-well cmd-view-toggle activity-switch"
				role="group"
				aria-label="Activity view"
			>
				<button
					type="button"
					aria-pressed={viewMode === 'assembly'}
					onclick={() => (viewMode = 'assembly')}
					class={[
						'cmd-toggle-btn min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11',
						viewMode === 'assembly' && 'is-on'
					]}
				>
					<Layers class="size-3.5" aria-hidden="true" />
					<span>Assembly line</span>
				</button>
				<button
					type="button"
					aria-pressed={viewMode === 'feed'}
					onclick={() => (viewMode = 'feed')}
					class={[
						'cmd-toggle-btn min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11',
						viewMode === 'feed' && 'is-on'
					]}
				>
					<Activity class="size-3.5" aria-hidden="true" />
					<span>Activity</span>
				</button>
			</div>

			<!-- Project Scope Select -->
			<div class="cmd-select-wrap activity-scope">
				<select
					aria-label="Project scope"
					value={effectiveProjectScope}
					onchange={(event) => (projectScope = event.currentTarget.value)}
					class="cmd-select min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
				>
					<option value="all">
						All projects ({model.activeProjects.length} active)
					</option>
					{#each model.activeProjects as project (project.id)}
						<option value={project.id}>
							{project.name}{#if project.key}
								· {project.key}{/if}
						</option>
					{/each}
				</select>
				<ChevronDown class="cmd-select-chevron size-3.5" aria-hidden="true" />
			</div>

			{#if viewMode === 'feed'}
				<!-- WAI-ARIA Tabs Pattern with roving tabindex and Arrow/Home/End navigation -->
				<div class="tabs activity-stream-tabs" role="tablist" aria-label="Filter activity stream">
					{#each streamTabs as tab, index (tab.id)}
						<button
							type="button"
							role="tab"
							id={`activity-tab-${tab.id}`}
							tabindex={streamFilter === tab.id ? 0 : -1}
							aria-selected={streamFilter === tab.id}
							aria-controls={`activity-panel-${tab.id}`}
							onclick={() => (streamFilter = tab.id)}
							onkeydown={(event) => handleStreamTabKeydown(event, index)}
							class={[
								'activity-stream-tab min-h-9 min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11',
								streamFilter === tab.id && 'is-active'
							]}
						>
							<span>{tab.label}</span>
							{#if tab.id === 'needs_you' && model.streamNeedsYouCount > 0}
								<span class="activity-stream-badge tabular-nums">{model.streamNeedsYouCount}</span>
							{/if}
						</button>
					{/each}
				</div>
			{/if}
		</div>
	</div>

	<!-- Collapsible Release Gates Dock + Slim Tool Rail (poc/operations.css 3c4e3541) -->
	<Tooltip.Provider delayDuration={150} ignoreNonKeyboardFocus>
		<div class="activity-body" data-panel-open={panelOpen ? '' : undefined}>
			<div class="activity-main" data-view={viewMode}>
				{#if viewMode === 'assembly'}
					<AssemblyLineMatrix
						{model}
						loading={!shellReady}
						unavailableReason={workUnavailableReason}
						{inboxPhase}
						onRetry={retryShellAndInbox}
					/>
				{:else}
					<div class="activity-stream-pad">
						<ActivityStreamPanel
							entries={model.streamEntries}
							{decisionEntries}
							needsYouCount={model.streamNeedsYouCount}
							openInboxCount={inboxPhase === 'ready' ? model.openInboxCount : 0}
							actionableWorkCount={workPhase === 'ready' ? model.actionableWorkCount : 0}
							bottleneck={model.bottleneck}
							bind:activeFilter={streamFilter}
							showTabs={false}
							{nowMs}
							{feedPhase}
							{feedError}
							{feedMore}
							{decisionPhase}
							{decisionError}
							{decisionMore}
							{inboxPhase}
							{workPhase}
							onRetry={retryFeed}
							onRetryDecisions={retryDecisions}
						/>
					</div>
				{/if}
			</div>

			<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
			<aside
				id="activity-release-panel"
				class="activity-panel"
				aria-label="Release gates and operational context"
				inert={!panelOpen ? true : undefined}
				onkeydown={handlePanelKeydown}
			>
				<div class="side-panel-header">
					<h2>Release gates</h2>
					<Tooltip.Root delayDuration={250} ignoreNonKeyboardFocus>
						<Tooltip.Trigger>
							{#snippet child({ props })}
								<button
									{...props}
									type="button"
									aria-label="Hide release gates"
									onclick={() => {
										panelOpen = false;
										toolsBtnEl?.focus();
									}}
									class="activity-tool-btn min-h-9 min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
								>
									<ChevronRight class="size-4" aria-hidden="true" />
								</button>
							{/snippet}
						</Tooltip.Trigger>
						<Tooltip.Portal>
							<Tooltip.Content
								side="left"
								sideOffset={6}
								escapeKeydownBehavior="defer-otherwise-close"
								class="activity-tooltip-content"
							>
								Hide release gates
							</Tooltip.Content>
						</Tooltip.Portal>
					</Tooltip.Root>
				</div>

				<ActivityOpsRail
					{model}
					loading={!shellReady}
					unavailableReason={workUnavailableReason}
					embedded
				/>
			</aside>

			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<div class="activity-tools" onkeydown={handlePanelKeydown}>
				<Tooltip.Root delayDuration={250} ignoreNonKeyboardFocus>
					<Tooltip.Trigger>
						{#snippet child({ props })}
							<button
								{...props}
								bind:this={toolsBtnEl}
								type="button"
								aria-expanded={panelOpen}
								aria-controls="activity-release-panel"
								aria-label={panelOpen ? 'Hide release gates' : 'Show release gates'}
								data-testid="activity-gates-toggle"
								onclick={() => (panelOpen = !panelOpen)}
								class="activity-tool-btn min-h-9 min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
							>
								<Flag class="size-4" aria-hidden="true" />
							</button>
						{/snippet}
					</Tooltip.Trigger>
					<Tooltip.Portal>
						<Tooltip.Content
							side="left"
							sideOffset={6}
							escapeKeydownBehavior="defer-otherwise-close"
							class="activity-tooltip-content"
						>
							{panelOpen ? 'Hide release gates' : 'Show release gates'}
						</Tooltip.Content>
					</Tooltip.Portal>
				</Tooltip.Root>
			</div>
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

	.section-header {
		flex: none;
	}

	.cmd-header {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 10px 20px;
		padding: 14px var(--activity-gutter) 8px;
	}

	.cmd-title-group {
		min-width: 0;
	}

	.cmd-h1 {
		color: var(--exec-ink);
		font-size: 1.5rem;
		font-weight: var(--exec-h1-weight, 650);
		letter-spacing: var(--exec-h1-tracking, -0.025em);
		line-height: 1.2;
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.cmd-subtitle {
		margin-top: 4px;
		color: var(--fg-secondary);
		font-size: 0.875rem;
		line-height: 1.4;
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.section-toolbar {
		position: relative;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px 12px;
		padding: 8px var(--activity-gutter);
		border-block-end: 1px solid var(--line);
	}

	.activity-stream-tabs {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 4px 12px;
		min-width: 0;
		max-width: 100%;
		padding-inline: 0;
		box-shadow: none;
	}

	.activity-stream-tab {
		position: relative;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 6px;
		padding: 0 8px;
		border: 0;
		border-bottom: 2px solid transparent;
		background: transparent;
		color: var(--fg-secondary);
		font-size: 0.8125rem;
		font-weight: 550;
		white-space: nowrap;
		cursor: pointer;
	}

	.activity-stream-tab:hover {
		color: var(--exec-ink);
	}

	.activity-stream-tab:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}

	.activity-stream-tab.is-active {
		color: var(--exec-ink);
		border-bottom-color: var(--tone-accent);
	}

	.activity-stream-badge {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: 18px;
		padding: 1px 6px;
		border-radius: var(--exec-pill-radius, 999px);
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
		color: var(--exec-warn);
		font-size: 0.6875rem;
		font-weight: 650;
	}

	.cmd-body {
		display: flex;
		flex-direction: column;
		gap: 18px;
		min-width: 0;
		padding: 18px var(--activity-gutter) 0;
	}

	.cmd-select-wrap {
		position: relative;
		display: inline-flex;
		align-items: center;
		max-width: 100%;
	}

	.cmd-select {
		appearance: none;
		max-width: 100%;
		padding: 0 32px 0 12px;
		border: 1px solid var(--line-strong);
		border-radius: var(--exec-ctrl-radius, 8px);
		background-color: var(--exec-well);
		color: var(--exec-ink);
		font-size: 0.8125rem;
		font-weight: 550;
		cursor: pointer;
	}

	.cmd-select:hover {
		background-color: var(--exec-surface-hi);
	}

	.cmd-select:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}

	.cmd-select-wrap :global(.cmd-select-chevron) {
		position: absolute;
		right: 10px;
		color: var(--fg-secondary);
		pointer-events: none;
	}

	.cmd-view-toggle {
		display: inline-flex;
		align-items: center;
		gap: 3px;
		padding: 3px;
	}

	.cmd-toggle-btn {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 0 12px;
		border: 0;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: transparent;
		color: var(--fg-secondary);
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
	}

	.cmd-toggle-btn:hover {
		color: var(--exec-ink);
	}

	.cmd-toggle-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}

	.cmd-toggle-btn.is-on {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1.5px var(--tone-accent-ink);
	}

	.cmd-status-slot {
		display: inline-flex;
		border-radius: var(--exec-pill-radius, 999px);
	}

	.cmd-status-slot:focus {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}

	.cmd-needs-pill {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		padding: 0 12px;
		border: 0;
		border-radius: var(--exec-pill-radius, 999px);
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		text-decoration: none;
	}

	button.cmd-needs-pill {
		cursor: pointer;
	}

	.cmd-needs-pill.is-neutral {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		color: var(--fg-secondary);
	}

	.cmd-needs-pill.is-warn {
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
		color: var(--exec-warn);
	}

	.cmd-needs-pill.is-warn:hover {
		box-shadow: inset 0 0 0 1.5px var(--exec-warn);
	}

	.cmd-needs-pill.is-ok {
		background: var(--exec-ok-bg);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
		color: var(--exec-ok);
	}

	.cmd-needs-pill:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}

	.cmd-needs-dot {
		width: 7px;
		height: 7px;
		border-radius: 999px;
		background: var(--exec-warn);
		flex-shrink: 0;
	}

	.cmd-live-banner {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 8px 12px;
		padding: 6px 12px;
		border: 1px solid var(--exec-border-hi);
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-well);
		color: var(--exec-body);
		font-size: 0.8125rem;
	}

	.cmd-live-banner:focus {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}

	.cmd-live-text {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		color: var(--exec-warn);
		font-weight: 550;
	}

	.cmd-live-reconnect {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		padding: 0 12px;
		border: 1px solid var(--line-strong);
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-surface);
		color: var(--tone-accent-ink);
		font-size: 0.8125rem;
		font-weight: 600;
		cursor: pointer;
	}

	.cmd-live-reconnect:disabled {
		opacity: 0.72;
		cursor: wait;
	}

	.cmd-live-reconnect:hover {
		background: var(--exec-surface-hi);
		color: var(--exec-ink);
	}

	.cmd-live-reconnect:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
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
	.activity-panel {
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
		visibility: hidden;
		overflow-x: hidden;
		z-index: 1;
		background: var(--side-panel-surface, var(--exec-surface));
		border-inline-start: 1px solid var(--line);
		box-shadow: var(--activity-panel-shadow);
	}

	.activity-body[data-panel-open] .activity-panel {
		visibility: visible;
	}

	.side-panel-header {
		position: sticky;
		inset-block-start: 0;
		z-index: 1;
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		padding: 8px var(--activity-panel-padding);
		border-block-end: 1px solid var(--line);
		background: var(--side-panel-surface, var(--exec-surface));
	}

	.side-panel-header h2 {
		margin: 0;
		color: var(--exec-ink);
		font-size: 0.9375rem;
		font-weight: 650;
	}

	.activity-tools {
		grid-area: tools;
		z-index: 2;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 4px;
		padding-block: 8px;
		border-inline-start: 1px solid var(--line);
		background: var(--side-panel-surface, var(--exec-surface));
	}

	.activity-tool-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		border: 0;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: transparent;
		color: var(--fg-secondary);
		cursor: pointer;
	}

	.activity-tool-btn:hover {
		background: var(--exec-well);
		color: var(--exec-ink);
	}

	.activity-tool-btn[aria-expanded='true'] {
		background: var(--exec-well);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--line-strong);
	}

	.activity-tool-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
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

	@media (width < 40rem) {
		.cmd-select-wrap {
			flex: 1 1 100%;
		}
		.cmd-select {
			width: 100%;
		}
	}

	:global(html[data-ui-theme^='report']) .activity-toolbar {
		border-block-end: 1.5px solid var(--report-masthead-rule);
	}

	@media (forced-colors: active) {
		.cmd-toggle-btn.is-on,
		.activity-stream-tab.is-active {
			border: 2px solid Highlight;
		}
		.cmd-needs-pill,
		.cmd-live-banner,
		.cmd-live-reconnect,
		.activity-tool-btn[aria-expanded='true'] {
			border: 1px solid CanvasText;
		}
	}
</style>
