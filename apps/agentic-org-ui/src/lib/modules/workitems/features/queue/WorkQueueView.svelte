<!--
  @component
  Unified Work queue shared by `/work` (organization scope) and `/projects/[id]/work` (project scope)
  (work-items-ux §6, §9, §11; feature-requests-ux §2; PRJ-1, WORK-1..10, TSK-1..10, BUG-10..15, RQS-7..15).
  Uses POC-aligned header, toolbar, Filter/Display icon popover menus, and Linear-style WorkList rows.
-->
<script lang="ts">
	import { tick, type Snippet } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { SvelteSet, SvelteURLSearchParams } from 'svelte/reactivity';
	import { Tooltip } from 'bits-ui';
	import { X } from '@lucide/svelte';
	import { toast } from 'svelte-sonner';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import WorkReadingPanel from '$lib/components/WorkReadingPanel.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { gutters, inputClass, mobileButton } from '$lib/components/classes';
	import InteriorSearchBar from '$lib/platform/shell/InteriorSearchBar.svelte';
	import { useShellCommand } from '$lib/platform/shell/shell-command';
	import { useShellData } from '$lib/platform/shell/shell-data.svelte';
	import { useScopedSearchContext } from '$lib/platform/shell/scoped-search.svelte';
	import { isEditableTarget, singleKeyShortcutsOn } from '$lib/platform/shell/shortcuts';
	import { uiPrefs } from '$lib/platform/shell/ui-preferences.svelte';
	import { workItemsApi } from '../../api';
	import {
		isArchivedTaskRow,
		readSessionShowArchived,
		writeSessionShowArchived
	} from '../../archive';
	import {
		SHOW_VIEWS,
		defaultFilters,
		defaultShowView,
		filterRows,
		hasActiveFilters,
		isShowView,
		mineRows,
		needsMeRows,
		readStoredDividers,
		readStoredLayout,
		readStoredTab,
		savedFilterCounts,
		writeStoredDividers,
		writeStoredLayout,
		writeStoredTab,
		type LayoutMode,
		type QueueFilters,
		type SavedFilterId,
		type ShowView
	} from '../../filters';
	import {
		TYPE_WORD,
		isOpen,
		normalizeRef,
		taskHref,
		type BoardMode,
		type GroupBy,
		type WorkSortBy,
		type WorkSortDir
	} from '../../present';
	import type { Priority, TaskRow, TaskStatus, TaskType, WorkQueue, WorkScope } from '../../types';
	import type { WorkStatusRingTone } from '$lib/modules/work/compact';
	import StaleBanner from '../../components/StaleBanner.svelte';
	import CaptureDialog from '../capture/CaptureDialog.svelte';
	import WorkDetailPage from '../detail/WorkDetailPage.svelte';
	import { buildActiveFilterChips, type ActiveFilterChip } from './active-chips';
	import WorkBoard from './WorkBoard.svelte';
	import WorkList from './WorkList.svelte';
	import WorkToolbarMenus from './WorkToolbarMenus.svelte';
	import NeedsMeList from './NeedsMeList.svelte';

	let {
		organizationId,
		scope,
		projectArchived = false,
		onRegisterToolbar = undefined
	}: {
		organizationId: string;
		scope: WorkScope;
		projectArchived?: boolean;
		onRegisterToolbar?: (controls: Snippet | undefined) => void;
	} = $props();

	const uid = $props.id();
	const isOrgScope = $derived(scope.kind === 'organization');
	const projectId = $derived(scope.kind === 'project' ? scope.projectId : null);
	const scopeKey = $derived(
		scope.kind === 'organization'
			? `org.${organizationId}`
			: `proj.${organizationId}.${scope.projectId}`
	);

	const queueRead = new Resource<WorkQueue>(() => workItemsApi.queue(organizationId, scope));

	const tabs: { id: TaskType; label: string }[] = [
		{ id: 'task', label: 'Tasks' },
		{ id: 'bug', label: 'Bugs' },
		{ id: 'request', label: 'Requests' }
	];

	let activeTab = $derived<TaskType>(isOrgScope ? readStoredTab(organizationId) : 'task');
	let layout = $derived<LayoutMode>(readStoredLayout(scopeKey));
	// Project Work lists default to grouping by Status with Blocked first (Prototype Release 53728c05).
	let taskGroupBy = $derived<GroupBy>(isOrgScope ? 'none' : 'status');
	let bugGroupBy = $state<GroupBy>('assigned_team');
	let requestGroupBy = $state<GroupBy>('status');
	let subgroupBy = $state<GroupBy>('none');
	let sortBy = $state<WorkSortBy>('default');
	let sortDir = $state<WorkSortDir>('asc');
	let showDividers = $state(readStoredDividers());
	let showArchived = $state(readSessionShowArchived());
	let boardMode = $state<BoardMode>('stages');
	let filters = $state<QueueFilters>(defaultFilters());
	let savedExplicit = $state(false);
	let showOverride = $state<ShowView | null>(null);

	let captureOpen = $state(false);
	let captureType = $state<TaskType>('task');
	let openRowRef = $state<string | null>(null);
	let savedQueueScrollTop = 0;
	let lastTriggerEl: HTMLElement | null = null;
	const activeOpenRow = $derived(
		openRowRef ? (queueRead.data?.rows.find((r) => r.ref === openRowRef) ?? null) : null
	);
	const shortcutsEnabled = $derived(singleKeyShortcutsOn());
	const shellCommand = useShellCommand();
	let shellData: ReturnType<typeof useShellData> | undefined;
	try {
		shellData = useShellData();
	} catch {
		shellData = undefined;
	}
	const scopedSearch = useScopedSearchContext();
	const usesHeaderScopedSearch = $derived(Boolean(scopedSearch));

	$effect(() => {
		if (!scopedSearch) return;
		return scopedSearch.register({
			getValue: () => filters.query,
			setValue: (next) => {
				filters = { ...filters, query: next };
				if (isOrgScope && activeShow === 'needs_me' && next.trim()) {
					void setShow('mine');
				}
			},
			label: isOrgScope ? 'Search work' : 'Search work in this view',
			placeholder: isOrgScope ? 'Search title or T- ref…' : 'Search work…',
			headingSelector: '#project-heading, #shell-page-title, main h1, main h2'
		});
	});

	function reloadQueue() {
		queueRead.reload();
		void shellData?.refresh();
	}

	function readingStatusTone(status: TaskStatus): WorkStatusRingTone {
		if (status === 'done' || status === 'cancelled') return 'done';
		if (status === 'blocked') return 'blocked';
		if (status === 'in_progress') return 'active';
		return 'neutral';
	}

	function readingPriority(priority: Priority): {
		level: 1 | 2 | 3;
		label: string;
		strong: boolean;
	} {
		if (priority === 'urgent') return { level: 3, label: 'Urgent priority', strong: true };
		if (priority === 'high') return { level: 3, label: 'High priority', strong: false };
		if (priority === 'normal') return { level: 2, label: 'Normal priority', strong: false };
		return { level: 1, label: 'Low priority', strong: false };
	}

	function openInlineDetail(row: TaskRow, triggerEl: HTMLElement | null) {
		const scroller = document.querySelector<HTMLElement>('main');
		savedQueueScrollTop = scroller ? scroller.scrollTop : window.scrollY;
		lastTriggerEl =
			triggerEl ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
		openRowRef = row.ref;
	}

	function closeInlineDetail(restoreFocus = true) {
		const closedRef = openRowRef;
		const trigger = lastTriggerEl;
		openRowRef = null;
		lastTriggerEl = null;
		if (!restoreFocus) return;
		void tick().then(() => {
			if (openRowRef) return;
			const scroller = document.querySelector<HTMLElement>('main');
			if (scroller && savedQueueScrollTop > 0) scroller.scrollTop = savedQueueScrollTop;
			if (trigger?.isConnected && trigger.offsetParent !== null) {
				trigger.focus({ preventScroll: true });
				trigger.scrollIntoView({ block: 'nearest', inline: 'nearest' });
				return;
			}
			if (closedRef) {
				const links = document.querySelectorAll<HTMLElement>(
					`[data-ref="${CSS.escape(closedRef)}"] a`
				);
				for (const link of links) {
					if (link.isConnected && link.offsetParent !== null) {
						link.focus({ preventScroll: true });
						link.scrollIntoView({ block: 'nearest', inline: 'nearest' });
						return;
					}
				}
			}
			document.getElementById('shell-page-title')?.focus({ preventScroll: true });
		});
	}

	// Deep links to /work?new=1 open the task composer and strip the parameter so reload doesn't reopen it.
	const wantsNewTask = $derived(isOrgScope && page.url.searchParams.get('new') === '1');
	$effect(() => {
		if (!wantsNewTask) return;
		openCapture('task');
		const nextUrl = new URL(page.url);
		nextUrl.searchParams.delete('new');
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- preserving current already-resolved URL + hash
		void goto(nextUrl, { replaceState: true, noScroll: true, keepFocus: true });
	});

	// Legacy /work?request=<id> links (Command search, Activity, Home) resolve directly to /work/[ref].
	const wantedRequest = $derived(isOrgScope ? page.url.searchParams.get('request') : null);
	$effect(() => {
		if (!wantedRequest || !queueRead.data) return;
		const norm = normalizeRef(wantedRequest);
		const match = queueRead.data.rows.find(
			(r) => r.id === wantedRequest || normalizeRef(r.ref) === norm
		);
		if (match) {
			void goto(resolve(taskHref(match.ref) as '/'), { replaceState: true });
			return;
		}
		toast('That request is no longer here.');
		const nextUrl = new URL(page.url);
		nextUrl.searchParams.delete('request');
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- preserving current already-resolved URL + hash
		void goto(nextUrl, { replaceState: true, noScroll: true, keepFocus: true });
	});

	function focusWorkSearchInput() {
		const headerInput = document.querySelector<HTMLInputElement>(
			'[data-testid="scoped-work-search"]'
		);
		if (headerInput && headerInput.offsetParent !== null) {
			headerInput.focus();
			return true;
		}
		const localInput = document.getElementById(`${uid}-search`) as HTMLInputElement | null;
		if (localInput) {
			localInput.focus();
			return true;
		}
		return false;
	}

	function isBareSlashEvent(event: KeyboardEvent, modalSelector: string): boolean {
		if (event.defaultPrevented || event.isComposing || event.keyCode === 229) return false;
		if (event.key !== '/' || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) {
			return false;
		}
		return !document.querySelector(modalSelector) && !isEditableTarget(event.target);
	}

	function focusSearchAfterShowSwitch() {
		const run = async () => {
			if (isOrgScope && activeShow === 'needs_me' && queueRead.data) {
				await setShow('mine');
			}
			await tick();
			focusWorkSearchInput();
		};
		void run();
	}

	function workSearchShortcut() {
		const modalOverlaySelector =
			'[role="dialog"]:not(.work-reading), [role="menu"], [role="alertdialog"], [role="listbox"]';
		const onSearchKey = (event: KeyboardEvent) => {
			if (!isBareSlashEvent(event, modalOverlaySelector)) return;
			const needsSwitch = isOrgScope && activeShow === 'needs_me' && Boolean(queueRead.data);
			if (!activeOpenRow && !needsSwitch && usesHeaderScopedSearch) return;
			if (!activeOpenRow && !needsSwitch && !document.getElementById(`${uid}-search`)) return;
			event.stopPropagation();
			if (!shortcutsEnabled || event.repeat) return;
			event.preventDefault();
			if (activeOpenRow) closeInlineDetail(false);
			if (activeOpenRow || needsSwitch) focusSearchAfterShowSwitch();
			else focusWorkSearchInput();
		};
		const onEscapeKey = (event: KeyboardEvent) => {
			if (
				event.defaultPrevented ||
				event.isComposing ||
				event.keyCode === 229 ||
				event.key !== 'Escape' ||
				event.repeat ||
				!activeOpenRow
			) {
				return;
			}
			if (document.querySelector(modalOverlaySelector)) return;
			if (
				event.target instanceof HTMLSelectElement ||
				(event.target instanceof Element &&
					Boolean(event.target.closest('select, [role="listbox"], [role="combobox"]')))
			) {
				return;
			}
			if (isEditableTarget(event.target)) return;
			event.preventDefault();
			closeInlineDetail(true);
		};
		window.addEventListener('keydown', onSearchKey, { capture: true });
		window.addEventListener('keydown', onEscapeKey);
		return () => {
			window.removeEventListener('keydown', onSearchKey, { capture: true });
			window.removeEventListener('keydown', onEscapeKey);
		};
	}

	function handleSearchKeydown(event: KeyboardEvent) {
		if (event.isComposing || event.keyCode === 229 || event.key !== 'Escape') return;
		event.preventDefault();
		const heading = document.querySelector<HTMLElement>(
			'#project-heading, #shell-page-title, main h1, main h2'
		);
		if (heading) {
			if (!heading.hasAttribute('tabindex')) heading.tabIndex = -1;
			heading.focus({ preventScroll: true });
		} else {
			(event.currentTarget as HTMLInputElement | null)?.blur();
		}
	}

	$effect(() => {
		const register = onRegisterToolbar;
		if (isOrgScope || !register) return;
		register(projectWorkToolbarControls);
		return () => {
			register(undefined);
		};
	});

	// Sync Show view and bookmarked `?project=` / `?assignee=` filters with URL on `/work`.
	const urlView = $derived(page.url.searchParams.get('view'));
	const urlProject = $derived(isOrgScope ? (page.url.searchParams.get('project') ?? '') : '');
	const urlAssignee = $derived(isOrgScope ? (page.url.searchParams.get('assignee') ?? '') : '');
	let lastSyncedUrlKey = '';
	$effect(() => {
		if (!isOrgScope) return;
		const key = `${urlProject}|${urlAssignee}`;
		if (key === lastSyncedUrlKey) return;
		lastSyncedUrlKey = key;
		if (filters.projectId !== urlProject || filters.assigneeId !== urlAssignee) {
			filters = { ...filters, projectId: urlProject, assigneeId: urlAssignee };
		}
	});

	function syncFilterUrl(projectIdVal: string, assigneeIdVal: string) {
		if (!isOrgScope) return;
		lastSyncedUrlKey = `${projectIdVal}|${assigneeIdVal}`;
		const nextUrl = new URL(page.url);
		if (projectIdVal) nextUrl.searchParams.set('project', projectIdVal);
		else nextUrl.searchParams.delete('project');
		if (assigneeIdVal) nextUrl.searchParams.set('assignee', assigneeIdVal);
		else nextUrl.searchParams.delete('assignee');
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- preserving current already-resolved URL + hash
		void goto(nextUrl, { replaceState: true, noScroll: true, keepFocus: true });
	}

	const activeShow = $derived.by<ShowView>(() => {
		if (!isOrgScope) return 'mine';
		if (isShowView(urlView)) return urlView;
		if (showOverride) return showOverride;
		if (urlProject || urlAssignee) return 'mine';
		return queueRead.data ? defaultShowView(queueRead.data) : 'mine';
	});

	function selectTab(next: TaskType) {
		activeTab = next;
		if (isOrgScope) writeStoredTab(organizationId, next);
		if (next !== 'bug' && filters.severity !== 'all') {
			filters = { ...filters, severity: 'all' };
		}
		if (isOrgScope && activeShow !== 'mine') {
			void setShow('mine');
		}
	}

	function setLayout(next: LayoutMode) {
		layout = next;
		writeStoredLayout(scopeKey, next);
	}

	function setDividers(next: boolean) {
		showDividers = next;
		writeStoredDividers(next);
	}

	function setShowArchived(next: boolean) {
		showArchived = next;
		writeSessionShowArchived(next);
	}

	function setShow(next: ShowView): Promise<void> {
		showOverride = next;
		if (next === 'mine') {
			savedExplicit = false;
			if (filters.saved !== 'all') filters = { ...filters, saved: 'all' };
		}
		const params = new SvelteURLSearchParams(page.url.searchParams);
		params.set('view', next);
		return goto(resolve(`${page.url.pathname}?${params.toString()}` as '/'), {
			replaceState: true,
			noScroll: true,
			keepFocus: true
		});
	}

	function handleTabKey(event: KeyboardEvent, index: number) {
		if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
		let next: number | undefined;
		if (event.key === 'ArrowRight') next = (index + 1) % tabs.length;
		else if (event.key === 'ArrowLeft') next = (index - 1 + tabs.length) % tabs.length;
		else if (event.key === 'Home') next = 0;
		else if (event.key === 'End') next = tabs.length - 1;
		if (next === undefined) return;
		event.preventDefault();
		selectTab(tabs[next].id);
		(event.currentTarget as HTMLElement | null)?.parentElement
			?.querySelector<HTMLButtonElement>(`#${CSS.escape(`${uid}-tab-${tabs[next].id}`)}`)
			?.focus();
	}

	function openCapture(kind: TaskType) {
		captureType = kind;
		captureOpen = true;
	}

	function clearFilters() {
		filters = defaultFilters();
		savedExplicit = false;
		if (isOrgScope && (urlProject || urlAssignee)) {
			syncFilterUrl('', '');
		}
	}

	function selectSaved(saved: SavedFilterId) {
		savedExplicit = true;
		filters = { ...filters, saved };
	}

	function patchFilters(patch: Partial<QueueFilters>) {
		filters = { ...filters, ...patch };
	}

	function setProjectFilter(val: string) {
		filters = { ...filters, projectId: val };
		syncFilterUrl(val, filters.assigneeId);
	}

	function setAssigneeFilter(val: string) {
		filters = { ...filters, assigneeId: val };
		syncFilterUrl(filters.projectId, val);
	}

	function removeFilterChip(chip: ActiveFilterChip) {
		filters = { ...filters, ...chip.clearPatch };
		if (chip.syncUrl) {
			syncFilterUrl(
				chip.syncUrl.projectId ?? filters.projectId,
				chip.syncUrl.assigneeId ?? filters.assigneeId
			);
		}
	}

	function setPrimaryGroup(next: GroupBy) {
		if (activeTab === 'bug') bugGroupBy = next;
		else if (activeTab === 'request') requestGroupBy = next;
		else taskGroupBy = next;
	}

	const hasExplicitOrgFilter = $derived(
		savedExplicit ||
			filters.teamId !== '' ||
			filters.assignedTeamId !== '' ||
			filters.assigneeId !== '' ||
			filters.reporterId !== '' ||
			filters.projectId !== ''
	);

	const uniqueProjects = $derived.by(() => {
		const map: Record<string, string> = {};
		for (const r of queueRead.data?.rows ?? []) {
			if (r.project) map[r.project.id] = r.project.name;
		}
		return Object.entries(map).sort((a, b) => a[1].localeCompare(b[1]));
	});

	const uniqueTeams = $derived.by(() => {
		const map: Record<string, string> = {};
		for (const r of queueRead.data?.rows ?? []) {
			map[r.team.id] = r.team.name;
			if (r.assignedTeam) map[r.assignedTeam.id] = r.assignedTeam.name;
		}
		return Object.entries(map).sort((a, b) => a[1].localeCompare(b[1]));
	});

	const uniquePeople = $derived.by(() => {
		const map: Record<string, string> = {};
		for (const r of queueRead.data?.rows ?? []) {
			if (r.assignee) map[r.assignee.id] = r.assignee.name;
			map[r.reporter.id] = r.reporter.name;
		}
		return Object.entries(map).sort((a, b) => a[1].localeCompare(b[1]));
	});

	const taskGroupOptions = $derived<GroupBy[]>(
		isOrgScope
			? ['none', 'status', 'priority', 'project', 'assigned_team', 'team', 'assignee']
			: ['none', 'status', 'priority', 'assigned_team', 'team', 'assignee']
	);
	const bugGroupOptions = $derived<GroupBy[]>(
		isOrgScope
			? ['assigned_team', 'severity', 'priority', 'status', 'project', 'team', 'assignee', 'none']
			: ['assigned_team', 'severity', 'priority', 'status', 'team', 'assignee', 'none']
	);
	const requestGroupOptions = $derived<GroupBy[]>(
		isOrgScope
			? ['status', 'assigned_team', 'project', 'team', 'assignee', 'none']
			: ['status', 'assigned_team', 'team', 'assignee', 'none']
	);

	// Derived queue metrics so the unified header and toolbar share one state cleanly
	const archivedIds = $derived.by(() => {
		const ids = new SvelteSet<string>();
		const threshold = uiPrefs.archiveAfterDays;
		if (threshold <= 0) return ids;
		for (const r of queueRead.data?.rows ?? []) {
			if (isArchivedTaskRow(r, threshold)) ids.add(r.id);
		}
		return ids;
	});
	const visibleQueueRows = $derived(
		showArchived || archivedIds.size === 0
			? (queueRead.data?.rows ?? [])
			: (queueRead.data?.rows ?? []).filter((r) => !archivedIds.has(r.id))
	);
	const openItemCount = $derived(visibleQueueRows.filter((r) => isOpen(r.status)).length);
	const needsMeCount = $derived(needsMeRows(visibleQueueRows).length);
	const showModeState = $derived<ShowView>(isOrgScope ? activeShow : 'mine');
	const scopeMineState = $derived(isOrgScope && showModeState === 'mine' && !hasExplicitOrgFilter);
	const savedCountsState = $derived(
		queueRead.data
			? savedFilterCounts(visibleQueueRows, queueRead.data.viewer, activeTab)
			: {
					all: 0,
					owned_by_my_team: 0,
					assigned_to_my_team: 0,
					assigned_to_me: 0,
					reported_by_me: 0,
					blocked: 0
				}
	);
	const primaryGroupState = $derived.by<GroupBy>(() => {
		if (isOrgScope && showModeState === 'by_project') return 'project';
		if (isOrgScope && showModeState === 'by_assignee') return 'assignee';
		if (activeTab === 'bug') return bugGroupBy;
		if (activeTab === 'request') return requestGroupBy;
		return layout === 'list' ? taskGroupBy : 'none';
	});
	const isListSurfaceState = $derived(
		(isOrgScope && (showModeState === 'by_project' || showModeState === 'by_assignee')) ||
			activeTab !== 'task' ||
			layout === 'list'
	);
	const groupOptionsState = $derived(
		activeTab === 'bug'
			? bugGroupOptions
			: activeTab === 'request'
				? requestGroupOptions
				: taskGroupOptions
	);
	const showGroupMenuState = $derived(
		isListSurfaceState && (!isOrgScope || showModeState === 'mine')
	);
	const subgroupCandidatesState = $derived.by<GroupBy[]>(() => {
		const base: GroupBy[] =
			activeTab === 'bug' && (!isOrgScope || showModeState === 'mine')
				? [
						'none',
						'status',
						'severity',
						'priority',
						...(isOrgScope ? (['project'] as GroupBy[]) : []),
						'assigned_team',
						'team',
						'assignee'
					]
				: [
						'none',
						'status',
						'priority',
						...(isOrgScope ? (['project'] as GroupBy[]) : []),
						'assigned_team',
						'team',
						'assignee'
					];
		return base.filter((g) => g === 'none' || g !== primaryGroupState);
	});
	const effectiveSubgroupState = $derived<GroupBy>(
		primaryGroupState === 'none' || !subgroupCandidatesState.includes(subgroupBy)
			? 'none'
			: subgroupBy
	);
	const activeChips = $derived(
		buildActiveFilterChips({
			filters,
			activeTab,
			projects: uniqueProjects,
			teams: uniqueTeams,
			people: uniquePeople
		})
	);
	const showLayoutToggleState = $derived(
		(!isOrgScope
			? !onRegisterToolbar
			: showModeState !== 'needs_me' &&
				showModeState !== 'by_project' &&
				showModeState !== 'by_assignee') && activeTab === 'task'
	);
</script>

{#snippet projectWorkToolbarControls()}
	{#if activeTab === 'task' && queueRead.state === 'ready'}
		<div
			class="project-work-controls"
			role="group"
			aria-label="Work view controls"
			data-testid="project-work-controls"
		>
			<div class="work-layout-toggle" role="group" aria-label="Layout">
				<button
					type="button"
					aria-pressed={layout === 'list'}
					onclick={() => setLayout('list')}
					class="queue-tab work-toggle-tab min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
					>List</button
				>
				<button
					type="button"
					aria-pressed={layout === 'board'}
					onclick={() => setLayout('board')}
					class="queue-tab work-toggle-tab min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
					>Board</button
				>
			</div>
			{#if layout === 'board'}
				<div
					role="group"
					aria-label="Board columns"
					data-testid="board-mode-toggle"
					class="inline-flex rounded-md bg-surface-raised p-0.5 ring-1 ring-line"
				>
					<button
						type="button"
						aria-pressed={boardMode === 'stages'}
						onclick={() => (boardMode = 'stages')}
						class="rounded-sm px-2.5 py-1 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11 {boardMode ===
						'stages'
							? 'bg-surface-overlay text-fg'
							: 'text-fg-secondary hover:text-fg'}">4 stages</button
					>
					<button
						type="button"
						aria-pressed={boardMode === 'statuses'}
						onclick={() => (boardMode = 'statuses')}
						class="rounded-sm px-2.5 py-1 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11 {boardMode ===
						'statuses'
							? 'bg-surface-overlay text-fg'
							: 'text-fg-secondary hover:text-fg'}">6 statuses</button
					>
				</div>
			{/if}
		</div>
	{/if}
{/snippet}

{#snippet workToolbarContent(q: WorkQueue)}
	<div
		class="primary-tab-row"
		data-page-menu={isOrgScope ? '' : undefined}
		aria-keyshortcuts={isOrgScope && shortcutsEnabled ? 'M' : undefined}
	>
		<div role="tablist" aria-label="Work" class="work-tablist">
			{#each tabs as t, index (t.id)}
				{@const selected = activeTab === t.id && (!isOrgScope || showModeState === 'mine')}
				<button
					id="{uid}-tab-{t.id}"
					type="button"
					role="tab"
					data-menu-label={t.label}
					aria-selected={selected}
					aria-controls="{uid}-panel"
					tabindex={activeTab === t.id ? 0 : -1}
					onclick={() => selectTab(t.id)}
					onkeydown={(e) => handleTabKey(e, index)}
					class="queue-tab work-primary-tab {selected ? 'is-selected' : ''}"
				>
					<span>{t.label}</span>
					<span class="queue-tab-count">
						{visibleQueueRows.filter((r) => r.type === t.id).length}
					</span>
				</button>
			{/each}
		</div>
	</div>

	<WorkToolbarMenus
		{isOrgScope}
		showMode={showModeState}
		{needsMeCount}
		{activeTab}
		{layout}
		{boardMode}
		{filters}
		scopeMine={scopeMineState}
		savedCounts={savedCountsState}
		hasFilterBadge={hasActiveFilters(filters)}
		isListSurface={isListSurfaceState}
		showGroupMenu={showGroupMenuState}
		primaryGroup={primaryGroupState}
		groupOptions={groupOptionsState}
		subgroupCandidates={subgroupCandidatesState}
		effectiveSubgroup={effectiveSubgroupState}
		{sortBy}
		{sortDir}
		{showDividers}
		{showArchived}
		{uniqueProjects}
		{uniqueTeams}
		{uniquePeople}
		showLayoutToggle={showLayoutToggleState}
		canCreate={isOrgScope || !projectArchived}
		createDisabled={Boolean(q.stale)}
		onShowChange={(next) => void setShow(next)}
		onLayoutChange={setLayout}
		onBoardModeChange={(next) => (boardMode = next)}
		onSavedSelect={selectSaved}
		onFiltersChange={patchFilters}
		onProjectChange={setProjectFilter}
		onAssigneeChange={setAssigneeFilter}
		onClearFilters={clearFilters}
		onGroupChange={setPrimaryGroup}
		onSubgroupChange={(next) => (subgroupBy = next)}
		onSortByChange={(next) => (sortBy = next)}
		onSortDirChange={(next) => (sortDir = next)}
		onDividersChange={setDividers}
		onShowArchivedChange={setShowArchived}
		onCreate={openCapture}
	/>
{/snippet}

<div class="work-page flex min-w-0 flex-1 flex-col" {@attach workSearchShortcut}>
	<Tooltip.Provider delayDuration={150}>
		{#if isOrgScope}
			<header class="work-header queue-header section-header">
				<div class="queue-headline">
					<div class="ph-t">
						<h1 id="shell-page-title" tabindex="-1" class="work-h1">Work</h1>
						{#if queueRead.data}
							<span role="status" class="queue-count">
								<i class={[openItemCount === 0 && 'is-ok']} aria-hidden="true"></i>
								<span class="tabular-nums">
									{openItemCount} open{#if needsMeCount > 0}
										· {needsMeCount} need you{/if}
								</span>
							</span>
						{/if}
					</div>

					{#if shellCommand}
						<div class="queue-search-wrap">
							<InteriorSearchBar class="w-full" />
						</div>
					{/if}
				</div>

				{#if queueRead.data}
					<div class="work-toolbar work-toolbar-bar queue-toolbar section-toolbar">
						{@render workToolbarContent(queueRead.data)}
					</div>
				{/if}
			</header>
		{/if}

		<Loaded
			resource={queueRead}
			label="work"
			error="Couldn't load work"
			place={isOrgScope ? 'Work' : undefined}
			area={isOrgScope ? 'Work' : undefined}
			errorLevel={isOrgScope ? 2 : 3}
		>
			{#snippet children(q)}
				{@const isStale = !!q.stale}
				{@const sourceRows = visibleQueueRows}
				{@const needsMeItems = needsMeRows(sourceRows)}
				{@const tabSelected = !isOrgScope || showModeState === 'mine'}
				{@const activeShowLabel = SHOW_VIEWS.find((v) => v.id === showModeState)?.label ?? 'Work'}
				{@const baseRows = scopeMineState ? mineRows(sourceRows, q.viewer) : sourceRows}
				{@const typeTotal = sourceRows.filter((r) => r.type === activeTab).length}
				{@const filtered = filterRows(
					baseRows,
					q.viewer,
					showModeState === 'by_project' || showModeState === 'by_assignee' ? 'all' : activeTab,
					filters
				)}
				{@const displayRows = filtered}
				{@const boardRows =
					activeTab === 'task' && layout === 'board' && boardMode === 'statuses'
						? filterRows(baseRows, q.viewer, 'all', filters)
						: displayRows}

				{#if q.stale}
					<StaleBanner
						stale={q.stale}
						retrying={queueRead.reloading}
						onretry={() => queueRead.reload()}
					/>
				{/if}

				<div
					class={[
						'flex min-w-0 flex-1 flex-col',
						!isOrgScope && 'gap-4 py-4',
						!isOrgScope && gutters
					]}
				>
					{#if !isOrgScope}
						<div class="work-toolbar work-project-toolbar">
							{@render workToolbarContent(q)}
						</div>
					{/if}

					<!-- Main panel (Cozy Glass matte canvas begins below the translucent header on Org Work) -->
					<div
						id="{uid}-panel"
						role={tabSelected ? 'tabpanel' : 'region'}
						aria-labelledby={tabSelected ? `${uid}-tab-${activeTab}` : undefined}
						aria-label={tabSelected ? undefined : activeShowLabel}
						class={[
							'flex min-w-0 flex-1 flex-col gap-3',
							isOrgScope && 'work-results-canvas py-4',
							isOrgScope && gutters
						]}
					>
						{#if isOrgScope && showModeState === 'needs_me'}
							<NeedsMeList
								rows={needsMeItems}
								sources={q.needsMeSources}
								onopen={isOrgScope ? openInlineDetail : undefined}
							/>
						{:else if q.rows.length === 0}
							<EmptyState
								level={3}
								title="No work yet"
								description="Capture a task, bug or request with an accountable team even when there are no projects."
							>
								{#snippet action()}
									<div class="flex flex-wrap justify-center gap-2">
										<Button
											type="button"
											class={mobileButton}
											disabled={isStale || projectArchived}
											onclick={() => openCapture('task')}>New task</Button
										>
										<Button
											type="button"
											variant="outline"
											class={mobileButton}
											disabled={isStale || projectArchived}
											onclick={() => openCapture('request')}>New request</Button
										>
									</div>
								{/snippet}
							</EmptyState>
						{:else}
							{#if !usesHeaderScopedSearch}
								<div class="relative w-full max-w-xs">
									<label for="{uid}-search" class="sr-only"
										>{isOrgScope ? 'Search work' : 'Search work in this view'}</label
									>
									<Input
										id="{uid}-search"
										type="search"
										bind:value={filters.query}
										onkeydown={handleSearchKeydown}
										placeholder="Search title or T- ref…"
										aria-keyshortcuts={shortcutsEnabled ? '/' : undefined}
										class={inputClass}
									/>
									<span id="{uid}-search-scope" class="sr-only">
										Searches shared titles, references, teams and projects.
									</span>
								</div>
							{/if}

							{#if !q.viewer.employee && (filters.saved === 'owned_by_my_team' || filters.saved === 'assigned_to_my_team' || filters.saved === 'assigned_to_me' || filters.saved === 'reported_by_me' || scopeMineState)}
								<div class="work-notice" data-testid="no-employee-note">
									<span>
										This account has no employee record, so "my" filters match nothing. Choose All
										or filter by team or person.
									</span>
									<button
										type="button"
										class="work-notice-action"
										onclick={() => selectSaved('all')}
									>
										Show all
									</button>
								</div>
							{/if}

							{#if activeChips.length > 0 || showArchived}
								<div class="work-active-filters" data-testid="active-filter-bar">
									{#if showArchived}
										<button
											type="button"
											class="work-filter-chip work-archived-chip"
											data-testid="work-showing-archived-chip"
											onclick={() => setShowArchived(false)}
										>
											<span>Showing archived</span>
											<X class="size-3 shrink-0 opacity-70" aria-hidden="true" />
										</button>
									{/if}
									{#each activeChips as chip (chip.id)}
										<button
											type="button"
											class="work-filter-chip"
											onclick={() => removeFilterChip(chip)}
										>
											<span>{chip.label}</span>
											<X class="size-3 shrink-0 opacity-70" aria-hidden="true" />
										</button>
									{/each}
									{#if activeChips.length > 0}
										<button type="button" class="work-clear-btn" onclick={clearFilters}>
											Clear filters
										</button>
									{/if}
								</div>
							{/if}

							<!-- Content: Show By project / By assignee OR active type view -->
							{#if isOrgScope && showModeState === 'by_project'}
								{#if filtered.length === 0}
									<EmptyState
										level={3}
										title="No items match these filters"
										description="Try clearing or adjusting your filters."
									>
										{#snippet action()}
											<Button
												type="button"
												variant="outline"
												class={mobileButton}
												onclick={clearFilters}>Clear filters</Button
											>
										{/snippet}
									</EmptyState>
								{:else}
									<WorkList
										rows={filtered}
										tab="all"
										groupBy="project"
										subgroupBy={effectiveSubgroupState}
										{sortBy}
										{sortDir}
										showProject={false}
										{showDividers}
										{archivedIds}
										selectedRef={openRowRef}
										onopen={isOrgScope ? openInlineDetail : undefined}
									/>
								{/if}
							{:else if isOrgScope && showModeState === 'by_assignee'}
								{#if filtered.length === 0}
									<EmptyState
										level={3}
										title="No items match these filters"
										description="Try clearing or adjusting your filters."
									>
										{#snippet action()}
											<Button
												type="button"
												variant="outline"
												class={mobileButton}
												onclick={clearFilters}>Clear filters</Button
											>
										{/snippet}
									</EmptyState>
								{:else}
									<WorkList
										rows={filtered}
										tab="all"
										groupBy="assignee"
										subgroupBy={effectiveSubgroupState}
										{sortBy}
										{sortDir}
										showProject={true}
										{showDividers}
										{archivedIds}
										selectedRef={openRowRef}
										onopen={isOrgScope ? openInlineDetail : undefined}
									/>
								{/if}
							{:else if typeTotal === 0}
								{#if activeTab === 'bug'}
									<EmptyState
										level={3}
										title="No bugs yet"
										description="Report one when something doesn't work."
									>
										{#snippet action()}
											<Button
												type="button"
												class={mobileButton}
												disabled={isStale || projectArchived}
												onclick={() => openCapture('bug')}>Report bug</Button
											>
										{/snippet}
									</EmptyState>
								{:else if activeTab === 'request'}
									<EmptyState
										level={3}
										title="No requests yet"
										description="Capture what you need and choose an accountable team."
									>
										{#snippet action()}
											<Button
												type="button"
												class={mobileButton}
												disabled={isStale || projectArchived}
												onclick={() => openCapture('request')}>New request</Button
											>
										{/snippet}
									</EmptyState>
								{:else}
									<EmptyState
										level={3}
										title="No tasks yet"
										description="Create a task with a title and accountable team."
									>
										{#snippet action()}
											<Button
												type="button"
												class={mobileButton}
												disabled={isStale || projectArchived}
												onclick={() => openCapture('task')}>New task</Button
											>
										{/snippet}
									</EmptyState>
								{/if}
							{:else if (activeTab === 'task' && layout === 'board' ? boardRows.length : displayRows.length) === 0}
								{@const noOpenBugs =
									activeTab === 'bug' &&
									filters.status === 'open' &&
									q.rows.filter((r) => r.type === 'bug' && isOpen(r.status)).length === 0}
								<EmptyState
									level={3}
									title={noOpenBugs
										? 'No open bugs.'
										: activeTab === 'bug'
											? 'No bugs match these filters.'
											: 'No items match these filters.'}
									description={noOpenBugs
										? 'Every recorded bug in this scope is closed.'
										: 'Clear filters to see all items.'}
								>
									{#snippet action()}
										{#if hasActiveFilters(filters)}
											<Button
												type="button"
												variant="outline"
												class={mobileButton}
												onclick={clearFilters}>Clear filters</Button
											>
										{:else if scopeMineState}
											<div class="flex flex-wrap justify-center gap-2">
												<Button
													type="button"
													variant="outline"
													class={mobileButton}
													onclick={() => selectSaved('all')}>Show all</Button
												>
												<Button
													type="button"
													variant="outline"
													class={mobileButton}
													onclick={() => void setShow('by_project')}>Show by project</Button
												>
											</div>
										{/if}
									{/snippet}
								</EmptyState>
							{:else if activeTab === 'task' && layout === 'board'}
								<WorkBoard
									{organizationId}
									rows={boardRows}
									{boardMode}
									showProject={isOrgScope}
									stale={isStale}
									{archivedIds}
									selectedRef={openRowRef}
									onchanged={reloadQueue}
									onopen={isOrgScope ? openInlineDetail : undefined}
								/>
							{:else}
								<WorkList
									rows={displayRows}
									tab={activeTab}
									groupBy={primaryGroupState}
									subgroupBy={effectiveSubgroupState}
									{sortBy}
									{sortDir}
									showProject={isOrgScope}
									{showDividers}
									{archivedIds}
									selectedRef={openRowRef}
									onopen={isOrgScope ? openInlineDetail : undefined}
								/>
							{/if}
						{/if}
					</div>
				</div>

				{#if isOrgScope && activeOpenRow}
					{@const pri = readingPriority(activeOpenRow.priority)}
					<div class="work-workspace has-work-detail">
						<WorkReadingPanel
							itemId={activeOpenRow.id}
							refLabel={activeOpenRow.ref}
							kindLabel={TYPE_WORD[activeOpenRow.type] as 'Task' | 'Bug' | 'Request'}
							ariaLabel="{activeOpenRow.ref} · {activeOpenRow.title}"
							fullPageHref={taskHref(activeOpenRow.ref)}
							statusTone={readingStatusTone(activeOpenRow.status)}
							needsAttention={Boolean(activeOpenRow.needsMe)}
							priorityLevel={pri.level}
							priorityLabel={pri.label}
							priorityStrong={pri.strong}
							onclose={() => closeInlineDetail(true)}
						>
							{#key activeOpenRow.ref}
								<WorkDetailPage
									{organizationId}
									refParam={activeOpenRow.ref}
									embedded
									onchanged={reloadQueue}
								/>
							{/key}
						</WorkReadingPanel>
					</div>
				{/if}
			{/snippet}
		</Loaded>
	</Tooltip.Provider>

	<CaptureDialog
		bind:open={captureOpen}
		{organizationId}
		initialType={captureType}
		{projectId}
		oncreated={reloadQueue}
		onadded={reloadQueue}
	/>
</div>

<style>
	.work-page {
		--work-gutter: 24px;
		--work-control-height: 36px;
	}
	@media (max-width: 720px), (pointer: coarse) {
		.work-page {
			--work-gutter: 16px;
			--work-control-height: 44px;
		}
	}
	.queue-header {
		position: relative;
		z-index: 1;
		container: queue-header / inline-size;
		border-bottom: 1px solid var(--exec-border, var(--line));
	}
	.queue-headline {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: flex-start;
		gap: 8px 16px;
		padding: 14px var(--work-gutter) 4px;
	}
	.ph-t {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 14px;
		min-width: 0;
	}
	.work-h1 {
		color: var(--exec-ink, var(--fg));
		font-size: clamp(1.375rem, 2.2vw, 1.875rem);
		line-height: 1.22;
		font-weight: 600;
		letter-spacing: -0.03em;
	}
	.work-h1:focus {
		outline: none;
	}
	.work-h1:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.queue-count {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.8125rem;
		font-weight: 500;
	}
	.queue-count i {
		display: inline-block;
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: var(--tone-accent, var(--brand));
		flex: none;
	}
	.queue-count i.is-ok {
		background: var(--tone-ok, var(--success));
	}
	.queue-search-wrap {
		display: flex;
		align-items: center;
		min-width: 0;
		max-width: 100%;
		margin-inline-start: auto;
	}
	.queue-toolbar {
		display: flex;
		flex-wrap: wrap;
		align-items: stretch;
		gap: 6px 8px;
		padding: 4px var(--work-gutter) 1px;
	}
	.work-project-toolbar {
		display: flex;
		flex-wrap: wrap;
		align-items: stretch;
		gap: 6px 8px;
		border-bottom: 1px solid var(--exec-border, var(--line));
	}
	.primary-tab-row {
		position: relative;
		display: flex;
		flex-wrap: wrap;
		align-items: stretch;
		gap: 2px 4px;
		min-inline-size: 0;
		max-inline-size: 100%;
	}
	.work-tablist {
		display: flex;
		flex-wrap: wrap;
		align-items: stretch;
		gap: 2px 4px;
	}
	.queue-tab {
		display: inline-flex;
		flex: none;
		align-items: center;
		gap: 6px;
		padding: 6px 12px 5px;
		min-height: var(--work-control-height);
		border: 1px solid transparent;
		border-block-end-width: 2px;
		border-radius: var(--exec-ctrl-radius, 6px) var(--exec-ctrl-radius, 6px) 0 0;
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.875rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.queue-tab:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
		color: var(--exec-ink, var(--fg));
	}
	.work-primary-tab.is-selected {
		border-block-end-color: var(--tone-accent-ink, var(--brand));
		color: var(--exec-ink, var(--fg));
	}
	.queue-tab:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -4px;
	}
	.queue-tab-count {
		color: var(--exec-muted, var(--fg-muted));
		font-variant-numeric: tabular-nums;
		font-size: 0.75rem;
	}
	.work-primary-tab.is-selected .queue-tab-count {
		color: var(--tone-accent-ink, var(--brand-text));
	}
	.work-notice {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 8px 12px;
		padding: 8px 12px;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line));
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.78125rem;
	}
	.work-notice-action {
		font-weight: 600;
		color: var(--tone-accent-ink, var(--link));
		cursor: pointer;
	}
	.work-notice-action:hover {
		text-decoration: underline;
	}
	.work-active-filters {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
	}
	.work-filter-chip {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		min-height: 26px;
		padding: 2px 9px;
		border-radius: 999px;
		background: var(--tone-accent-bg, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring, var(--line));
		color: var(--exec-ink, var(--fg));
		font-size: 0.75rem;
		font-weight: 500;
		cursor: pointer;
	}
	.work-filter-chip:hover {
		background: var(--exec-surface-hi, var(--surface-overlay));
	}
	.work-clear-btn {
		display: inline-flex;
		align-items: center;
		min-height: 26px;
		padding: 2px 8px;
		border-radius: var(--exec-ctrl-radius, 6px);
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.75rem;
		font-weight: 600;
		cursor: pointer;
	}
	.work-clear-btn:hover {
		color: var(--exec-ink, var(--fg));
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
	}
	.project-work-controls {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
		max-inline-size: 100%;
	}
	.work-layout-toggle {
		display: flex;
		align-items: center;
		gap: 2px;
		padding-inline-start: 8px;
		border-inline-start: 1px solid var(--exec-border, var(--line));
	}
	.work-toggle-tab {
		display: inline-flex;
		align-items: center;
		padding: 0 10px;
		border: 1px solid transparent;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
	}
	.work-toggle-tab:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
		color: var(--exec-ink, var(--fg));
	}
	.work-toggle-tab[aria-pressed='true'] {
		background: var(--workspace-mode-selected, var(--exec-well, var(--surface-raised)));
		border-color: var(--exec-border-hi, var(--line-strong));
		color: var(--exec-ink, var(--fg));
	}

	@container queue-header (min-width: 52rem) {
		.queue-headline {
			display: grid;
			grid-template-columns: minmax(0, 1fr) clamp(20rem, 34cqi, 30rem);
			align-items: center;
		}
		.queue-search-wrap {
			width: 100%;
			margin-inline: 0;
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
	}
	@container queue-header (max-width: 38rem) {
		.queue-tab {
			gap: 4px;
			padding-inline: 8px;
		}
	}

	@media (forced-colors: active) {
		.work-primary-tab {
			border-block-end-color: Canvas;
		}
		.work-primary-tab.is-selected {
			border-block-end-color: Highlight;
		}
		.work-toggle-tab[aria-pressed='true'] {
			background: Highlight;
			border-color: Highlight;
			color: HighlightText;
		}
	}
</style>
