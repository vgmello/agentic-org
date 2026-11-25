<!--
  @component
  Unified Work queue shared by `/work` (organization scope) and `/projects/[id]/work` (project scope)
  (work-items-ux §6, §9, §11; feature-requests-ux §2; PRJ-1, WORK-1..10, TSK-1..10, BUG-10..15, RQS-7..15).
  Uses Resource + Loaded (handling loading, unavailable, unsupported, error, ready).
-->
<script lang="ts">
	import { tick, type Snippet } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { SvelteURLSearchParams } from 'svelte/reactivity';
	import { toast } from 'svelte-sonner';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import WorkReadingPanel from '$lib/components/WorkReadingPanel.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { gutters, inputClass, mobileButton, selectClass } from '$lib/components/classes';
	import { useShellData } from '$lib/platform/shell/shell-data.svelte';
	import { useScopedSearchContext } from '$lib/platform/shell/scoped-search.svelte';
	import { isEditableTarget, singleKeyShortcutsOn } from '$lib/platform/shell/shortcuts';
	import { workItemsApi } from '../../api';
	import {
		SAVED_FILTERS,
		SHOW_VIEWS,
		defaultFilters,
		defaultShowView,
		filterRows,
		hasActiveFilters,
		isShowView,
		mineRows,
		needsMeRows,
		readStoredLayout,
		readStoredTab,
		savedFilterCounts,
		writeStoredLayout,
		writeStoredTab,
		type LayoutMode,
		type QueueFilters,
		type ShowView
	} from '../../filters';
	import {
		GROUP_WORD,
		PRIORITIES,
		PRIORITY_WORD,
		SEVERITIES,
		SEVERITY_WORD,
		STATUSES,
		STATUS_WORD,
		TYPE_WORD,
		isOpen,
		normalizeRef,
		severityWord,
		taskHref,
		type BoardMode,
		type GroupBy
	} from '../../present';
	import type { Priority, TaskRow, TaskStatus, TaskType, WorkQueue, WorkScope } from '../../types';
	import type { WorkStatusRingTone } from '$lib/modules/work/compact';
	import StaleBanner from '../../components/StaleBanner.svelte';
	import CaptureDialog from '../capture/CaptureDialog.svelte';
	import WorkDetailPage from '../detail/WorkDetailPage.svelte';
	import WorkBoard from './WorkBoard.svelte';
	import WorkList from './WorkList.svelte';
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
	const inlineSelect = selectClass.replace('w-full', 'w-auto');
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
	let shellData: ReturnType<typeof useShellData> | undefined;
	try {
		shellData = useShellData();
	} catch {
		shellData = undefined;
	}
	const scopedSearch = useScopedSearchContext();
	const usesHeaderScopedSearch = $derived(!isOrgScope && Boolean(scopedSearch));

	$effect(() => {
		if (!scopedSearch || isOrgScope) return;
		return scopedSearch.register({
			getValue: () => filters.query,
			setValue: (next) => {
				filters = { ...filters, query: next };
			},
			label: 'Search work in this view',
			placeholder: 'Search work…',
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

	// Home's "New task" CTA links to /work?new=1: open the task composer and strip the parameter so reload doesn't reopen it.
	const wantsNewTask = $derived(isOrgScope && page.url.searchParams.get('new') === '1');
	$effect(() => {
		if (!wantsNewTask) return;
		openCapture('task');
		const nextUrl = new URL(page.url);
		nextUrl.searchParams.delete('new');
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- preserving current already-resolved URL + hash
		void goto(nextUrl, {
			replaceState: true,
			noScroll: true,
			keepFocus: true
		});
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
		void goto(nextUrl, {
			replaceState: true,
			noScroll: true,
			keepFocus: true
		});
	});

	function workSearchShortcut() {
		const modalOverlaySelector =
			'[role="dialog"]:not(.work-reading), [role="menu"], [role="alertdialog"], [role="listbox"]';
		// Capture-phase `/` intercepts before ExecutiveCommandBar's global shortcut.
		const onSearchKey = (event: KeyboardEvent) => {
			if (event.defaultPrevented || event.isComposing || event.keyCode === 229) return;
			if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey || event.key !== '/') {
				return;
			}
			if (document.querySelector(modalOverlaySelector)) return;
			if (isEditableTarget(event.target)) return;
			if (activeOpenRow) {
				event.stopPropagation();
				if (!shortcutsEnabled || event.repeat) return;
				event.preventDefault();
				closeInlineDetail(false);
				const focusSearch = async () => {
					if (isOrgScope && activeShow === 'needs_me' && queueRead.data) {
						await setShow('mine');
					}
					await tick();
					(document.getElementById(`${uid}-search`) as HTMLInputElement | null)?.focus();
				};
				void focusSearch();
				return;
			}
			const searchEl = document.getElementById(`${uid}-search`) as HTMLInputElement | null;
			if (!searchEl && !(isOrgScope && activeShow === 'needs_me' && queueRead.data)) return;
			event.stopPropagation();
			if (!shortcutsEnabled || event.repeat) return;
			event.preventDefault();
			if (searchEl) {
				searchEl.focus();
				return;
			}
			void setShow('mine').then(async () => {
				await tick();
				(document.getElementById(`${uid}-search`) as HTMLInputElement | null)?.focus();
			});
		};
		// Bubble-phase Escape runs after Bits UI's document-level EscapeLayer so open tooltips,
		// menus, or dialogs consume the first Escape and keep the reading panel open.
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
		// Prototype Release 1149b4f1: Escape leaves the typed query intact and exits the field.
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
		void goto(nextUrl, {
			replaceState: true,
			noScroll: true,
			keepFocus: true
		});
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
		// Reset type-specific severity filter when leaving Bugs.
		if (next !== 'bug' && filters.severity !== 'all') {
			filters = { ...filters, severity: 'all' };
		}
		// On /work, clicking a type tab switches Show out of cross-type views so the chosen tab renders.
		if (isOrgScope && activeShow !== 'mine') {
			setShow('mine');
		}
	}

	function setLayout(next: LayoutMode) {
		layout = next;
		writeStoredLayout(scopeKey, next);
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

	function showAllSaved() {
		savedExplicit = true;
		filters = { ...filters, saved: 'all' };
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

<div class="work-page flex min-w-0 flex-1 flex-col" {@attach workSearchShortcut}>
	{#if isOrgScope}
		<div inert={activeOpenRow ? true : undefined}>
			<PageHeader
				title="Work"
				level={1}
				class="work-header queue-header"
				count={queueRead.data?.rows.length}
				countLabel="items"
				description="Tasks, bugs and requests across projects and teams."
			>
				{#snippet actions()}
					{#if queueRead.data}
						<Button
							type="button"
							variant="outline"
							class="{mobileButton} [@media(width<=720px)]:min-h-11"
							disabled={!!queueRead.data.stale}
							onclick={() => openCapture('request')}>New request</Button
						>
						<Button
							type="button"
							variant="outline"
							class="{mobileButton} [@media(width<=720px)]:min-h-11"
							disabled={!!queueRead.data.stale}
							onclick={() => openCapture('bug')}>Report bug</Button
						>
						<Button
							type="button"
							class="{mobileButton} [@media(width<=720px)]:min-h-11"
							disabled={!!queueRead.data.stale}
							onclick={() => openCapture('task')}>New task</Button
						>
					{/if}
				{/snippet}
			</PageHeader>
		</div>
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
			{@const needsMeItems = needsMeRows(q.rows)}
			{@const showMode = isOrgScope ? activeShow : 'mine'}
			{@const tabSelected = !isOrgScope || showMode === 'mine'}
			{@const activeShowLabel = SHOW_VIEWS.find((v) => v.id === showMode)?.label ?? 'Work'}
			{@const scopeMine = isOrgScope && showMode === 'mine' && !hasExplicitOrgFilter}
			{@const baseRows = scopeMine ? mineRows(q.rows, q.viewer) : q.rows}
			{@const savedCounts = savedFilterCounts(q.rows, q.viewer, activeTab)}
			{@const typeTotal = q.rows.filter((r) => r.type === activeTab).length}
			{@const filtered = filterRows(
				baseRows,
				q.viewer,
				showMode === 'by_project' || showMode === 'by_assignee' ? 'all' : activeTab,
				filters
			)}
			{@const displayRows = filtered}
			{@const primaryGroup =
				isOrgScope && showMode === 'by_project'
					? ('project' as GroupBy)
					: isOrgScope && showMode === 'by_assignee'
						? ('assignee' as GroupBy)
						: activeTab === 'bug'
							? bugGroupBy
							: activeTab === 'request'
								? requestGroupBy
								: layout === 'list'
									? taskGroupBy
									: ('none' as GroupBy)}
			{@const isListSurface =
				(isOrgScope && (showMode === 'by_project' || showMode === 'by_assignee')) ||
				activeTab !== 'task' ||
				layout === 'list'}
			{@const subgroupCandidates = (
				activeTab === 'bug' && (!isOrgScope || showMode === 'mine')
					? ([
							'none',
							'status',
							'severity',
							'priority',
							...(isOrgScope ? (['project'] as GroupBy[]) : []),
							'assigned_team',
							'team',
							'assignee'
						] as GroupBy[])
					: ([
							'none',
							'status',
							'priority',
							...(isOrgScope ? (['project'] as GroupBy[]) : []),
							'assigned_team',
							'team',
							'assignee'
						] as GroupBy[])
			).filter((g) => g === 'none' || g !== primaryGroup)}
			{@const effectiveSubgroup =
				primaryGroup === 'none' || !subgroupCandidates.includes(subgroupBy)
					? ('none' as GroupBy)
					: subgroupBy}
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
					!isOrgScope && 'gap-5 py-6',
					!isOrgScope && gutters
				]}
				inert={isOrgScope && activeOpenRow ? true : undefined}
			>
				<!-- Top controls: Type tabs + Project actions / Show menu / Board-List toggle -->
				<div
					class={[
						'work-toolbar-bar flex flex-wrap items-stretch justify-between gap-x-3 gap-y-1.5 border-b border-line',
						isOrgScope && gutters,
						isOrgScope && 'pt-2'
					]}
				>
					<div
						class="primary-tab-row"
						data-page-menu={isOrgScope ? '' : undefined}
						aria-keyshortcuts={isOrgScope && shortcutsEnabled ? 'M' : undefined}
					>
						<div role="tablist" aria-label="Work" class="flex flex-wrap gap-x-1">
							{#each tabs as t, index (t.id)}
								{@const selected = activeTab === t.id && (!isOrgScope || showMode === 'mine')}
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
									class="work-primary-tab inline-flex min-h-9 items-center gap-1.5 rounded-t-md border-b-2 px-3 pt-1.5 pb-[5px] text-sm/6 font-semibold whitespace-nowrap [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11 {selected
										? 'is-selected'
										: ''}"
								>
									<span>{t.label}</span>
									<span class="work-tab-count text-xs/5 font-normal tabular-nums">
										{q.rows.filter((r) => r.type === t.id).length}
									</span>
								</button>
							{/each}
						</div>
					</div>

					<div class="my-1 flex flex-wrap items-center gap-2 self-center">
						{#if isOrgScope}
							<label class="flex items-center gap-2 text-xs/5 text-fg-muted">
								<span>Show</span>
								<select
									aria-label="Show"
									value={showMode}
									onchange={(e) =>
										setShow((e.currentTarget as HTMLSelectElement).value as ShowView)}
									class={inlineSelect}
									data-testid="show-select"
								>
									{#each SHOW_VIEWS as v (v.id)}
										<option value={v.id}>
											{v.label}{v.id === 'needs_me' ? ` (${needsMeItems.length})` : ''}
										</option>
									{/each}
								</select>
							</label>
						{/if}

						{#if (!isOrgScope ? !onRegisterToolbar : showMode !== 'needs_me' && showMode !== 'by_project' && showMode !== 'by_assignee') && activeTab === 'task'}
							<div
								role="group"
								aria-label="Layout"
								class="inline-flex rounded-md bg-surface-raised p-0.5 ring-1 ring-line"
							>
								<button
									type="button"
									aria-pressed={layout === 'list'}
									onclick={() => setLayout('list')}
									class="rounded-sm px-2.5 py-1 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11 {layout ===
									'list'
										? 'bg-surface-overlay text-fg'
										: 'text-fg-secondary hover:text-fg'}">List</button
								>
								<button
									type="button"
									aria-pressed={layout === 'board'}
									onclick={() => setLayout('board')}
									class="rounded-sm px-2.5 py-1 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11 {layout ===
									'board'
										? 'bg-surface-overlay text-fg'
										: 'text-fg-secondary hover:text-fg'}">Board</button
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
						{/if}

						{#if !isOrgScope && !projectArchived}
							<Button
								type="button"
								variant="outline"
								size="sm"
								class={mobileButton}
								disabled={isStale}
								onclick={() => openCapture('request')}>New request</Button
							>
							<Button
								type="button"
								variant="outline"
								size="sm"
								class={mobileButton}
								disabled={isStale}
								onclick={() => openCapture('bug')}>Report bug</Button
							>
							<Button
								type="button"
								size="sm"
								class={mobileButton}
								disabled={isStale}
								onclick={() => openCapture('task')}>New task</Button
							>
						{/if}
					</div>
				</div>

				<!-- Main panel (Cozy Glass matte canvas begins below the translucent toolbar on Org Work) -->
				<div
					id="{uid}-panel"
					role={tabSelected ? 'tabpanel' : 'region'}
					aria-labelledby={tabSelected ? `${uid}-tab-${activeTab}` : undefined}
					aria-label={tabSelected ? undefined : activeShowLabel}
					class={[
						'flex min-w-0 flex-1 flex-col gap-4',
						isOrgScope && 'work-results-canvas py-5',
						isOrgScope && gutters
					]}
				>
					{#if isOrgScope && showMode === 'needs_me'}
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
						<!-- Saved filters (WORK-2, RQS-15) -->
						{#if !isOrgScope || (showMode !== 'by_project' && showMode !== 'by_assignee')}
							<nav
								aria-label="Saved filters"
								class="flex flex-wrap items-center gap-1.5"
								data-testid="saved-filters"
							>
								{#each SAVED_FILTERS as sf (sf.id)}
									{@const pressed = !scopeMine && filters.saved === sf.id}
									{@const label =
										activeTab === 'request' && sf.requestLabel ? sf.requestLabel : sf.label}
									<button
										type="button"
										aria-pressed={pressed}
										data-filter={sf.id}
										onclick={() => {
											savedExplicit = true;
											filters = { ...filters, saved: sf.id };
										}}
										class="inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11 {pressed
											? 'bg-primary text-on-emphasis'
											: 'bg-surface-raised text-fg-secondary ring-1 ring-line hover:bg-surface-overlay hover:text-fg'}"
									>
										<span>{label}</span>
										<span class="tabular-nums opacity-80">{savedCounts[sf.id]}</span>
									</button>
								{/each}
							</nav>

							{#if !q.viewer.employee && (filters.saved === 'owned_by_my_team' || filters.saved === 'assigned_to_my_team' || filters.saved === 'assigned_to_me' || filters.saved === 'reported_by_me' || scopeMine)}
								<p class="text-xs/5 text-fg-muted" data-testid="no-employee-note">
									This account has no employee record, so "my" filters match nothing. Choose All or
									filter by team or person.
								</p>
							{/if}
						{/if}

						<!-- Filter bar (WORK-1, TRI-7, BUG-12, BUG-13, RQS-15) -->
						<div class="flex flex-wrap items-center gap-2.5" data-testid="filter-bar">
							{#if !usesHeaderScopedSearch}
								<div class="relative w-full min-[640px]:w-60">
									<label for="{uid}-search" class="sr-only"
										>{isOrgScope ? 'Search work' : 'Search work in this view'}</label
									>
									<Input
										id="{uid}-search"
										type="search"
										bind:value={filters.query}
										onkeydown={handleSearchKeydown}
										placeholder="Search title or T- ref…"
										aria-describedby="{uid}-search-scope"
										aria-keyshortcuts={shortcutsEnabled ? '/' : undefined}
										class="{inputClass} {shortcutsEnabled && !filters.query ? 'pr-8' : ''}"
									/>
									{#if shortcutsEnabled && !filters.query}
										<kbd
											aria-hidden="true"
											class="pointer-events-none absolute top-1/2 right-2 hidden -translate-y-1/2 rounded border border-line-strong px-1.5 py-0.5 font-mono text-[11px]/none text-fg-muted min-[721px]:inline"
											>/</kbd
										>
									{/if}
									<span id="{uid}-search-scope" class="sr-only">
										Searches shared titles, references, teams and projects.
									</span>
								</div>
							{/if}

							<select
								aria-label="Filter by status"
								bind:value={filters.status}
								class={inlineSelect}
							>
								<option value="all">All statuses</option>
								<option value="open">Open</option>
								{#each STATUSES as s (s)}
									<option value={s}>{STATUS_WORD[s]}</option>
								{/each}
							</select>

							{#if activeTab === 'bug' && (!isOrgScope || showMode === 'mine')}
								<select
									aria-label="Filter by severity"
									bind:value={filters.severity}
									class={inlineSelect}
								>
									<option value="all">All severities</option>
									{#each SEVERITIES as sev (sev)}
										<option value={sev}>{SEVERITY_WORD[sev]}</option>
									{/each}
									<option value="unset">{severityWord(null)}</option>
								</select>
							{/if}

							<select
								aria-label="Filter by priority"
								bind:value={filters.priority}
								class={inlineSelect}
							>
								<option value="all">All priorities</option>
								{#each PRIORITIES as p (p)}
									<option value={p}>{PRIORITY_WORD[p]}</option>
								{/each}
							</select>

							{#if isOrgScope}
								<select
									aria-label="Filter by project"
									value={filters.projectId}
									onchange={(e) => {
										const val = (e.currentTarget as HTMLSelectElement).value;
										filters = { ...filters, projectId: val };
										syncFilterUrl(val, filters.assigneeId);
									}}
									class={inlineSelect}
								>
									<option value="">All projects</option>
									<option value="none">No project</option>
									{#each uniqueProjects as [id, name] (id)}
										<option value={id}>{name}</option>
									{/each}
								</select>
							{/if}

							<select
								aria-label="Filter by accountable team"
								bind:value={filters.teamId}
								class={inlineSelect}
							>
								<option value="">Accountable team</option>
								{#each uniqueTeams as [id, name] (id)}
									<option value={id}>{name}</option>
								{/each}
							</select>

							<select
								aria-label="Filter by assigned team"
								bind:value={filters.assignedTeamId}
								class={inlineSelect}
							>
								<option value="">Assigned team</option>
								<option value="none">Unassigned team</option>
								{#each uniqueTeams as [id, name] (id)}
									<option value={id}>{name}</option>
								{/each}
							</select>

							<select
								aria-label="Filter by assignee"
								value={filters.assigneeId}
								onchange={(e) => {
									const val = (e.currentTarget as HTMLSelectElement).value;
									filters = { ...filters, assigneeId: val };
									syncFilterUrl(filters.projectId, val);
								}}
								class={inlineSelect}
							>
								<option value="">Assignee</option>
								<option value="none">Unassigned</option>
								{#each uniquePeople as [id, name] (id)}
									<option value={id}>{name}</option>
								{/each}
							</select>

							<select
								aria-label="Filter by reporter"
								bind:value={filters.reporterId}
								class={inlineSelect}
							>
								<option value="">Reporter</option>
								{#each uniquePeople as [id, name] (id)}
									<option value={id}>{name}</option>
								{/each}
							</select>

							{#if activeTab === 'task' && layout === 'list' && (!isOrgScope || showMode === 'mine')}
								<select
									aria-label="Group tasks by"
									bind:value={taskGroupBy}
									class={inlineSelect}
									data-testid="group-by-select"
								>
									{#each taskGroupOptions as g (g)}
										<option value={g}>Group: {GROUP_WORD[g]}</option>
									{/each}
								</select>
							{:else if activeTab === 'bug' && (!isOrgScope || showMode === 'mine')}
								<select
									aria-label="Group bugs by"
									bind:value={bugGroupBy}
									class={inlineSelect}
									data-testid="group-by-select"
								>
									{#each bugGroupOptions as g (g)}
										<option value={g}>Group: {GROUP_WORD[g]}</option>
									{/each}
								</select>
							{:else if activeTab === 'request' && (!isOrgScope || showMode === 'mine')}
								<select
									aria-label="Group requests by"
									bind:value={requestGroupBy}
									class={inlineSelect}
									data-testid="group-by-select"
								>
									{#each requestGroupOptions as g (g)}
										<option value={g}>Group: {GROUP_WORD[g]}</option>
									{/each}
								</select>
							{/if}

							{#if isListSurface && primaryGroup !== 'none'}
								<select
									aria-label="Subgroup by"
									value={effectiveSubgroup}
									onchange={(e) => {
										subgroupBy = (e.currentTarget as HTMLSelectElement).value as GroupBy;
									}}
									class={inlineSelect}
									data-testid="subgroup-by-select"
								>
									{#each subgroupCandidates as g (g)}
										<option value={g}>Subgroup: {GROUP_WORD[g]}</option>
									{/each}
								</select>
							{/if}

							{#if hasActiveFilters(filters)}
								<Button
									type="button"
									variant="outline"
									size="sm"
									class={mobileButton}
									onclick={clearFilters}>Clear filters</Button
								>
							{/if}
						</div>

						<!-- Content: Show By project / By assignee OR active type view -->
						{#if isOrgScope && showMode === 'by_project'}
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
									subgroupBy={effectiveSubgroup}
									showProject={false}
									onopen={isOrgScope ? openInlineDetail : undefined}
								/>
							{/if}
						{:else if isOrgScope && showMode === 'by_assignee'}
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
									subgroupBy={effectiveSubgroup}
									showProject={true}
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
									{:else if scopeMine}
										<div class="flex flex-wrap justify-center gap-2">
											<Button
												type="button"
												variant="outline"
												class={mobileButton}
												onclick={showAllSaved}>Show all</Button
											>
											<Button
												type="button"
												variant="outline"
												class={mobileButton}
												onclick={() => setShow('by_project')}>Show by project</Button
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
								onchanged={reloadQueue}
								onopen={isOrgScope ? openInlineDetail : undefined}
							/>
						{:else}
							<WorkList
								rows={displayRows}
								tab={activeTab}
								groupBy={primaryGroup}
								subgroupBy={effectiveSubgroup}
								showProject={isOrgScope}
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
	.work-primary-tab {
		border-bottom-color: transparent;
		color: var(--exec-body, var(--fg-secondary));
		cursor: pointer;
	}
	.work-primary-tab:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
		color: var(--exec-ink, var(--fg));
	}
	.work-primary-tab.is-selected {
		border-bottom-color: var(--tone-accent-ink);
		color: var(--exec-ink, var(--fg));
	}
	.work-primary-tab:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -4px;
	}
	.work-tab-count {
		color: var(--exec-muted, var(--fg-muted));
	}
	.work-primary-tab.is-selected .work-tab-count {
		color: var(--tone-accent-ink);
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
		border-inline-start: 1px solid var(--line);
	}
	.work-toggle-tab {
		display: inline-flex;
		align-items: center;
		padding: 0 10px;
		border: 1px solid transparent;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: transparent;
		color: var(--fg-secondary);
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
	}
	.work-toggle-tab:hover {
		background: var(--surface-raised);
		color: var(--fg);
	}
	.work-toggle-tab[aria-pressed='true'] {
		background: var(--surface-raised);
		border-color: var(--line-strong);
		color: var(--fg);
	}
	.work-toggle-tab:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	@media (forced-colors: active) {
		.work-primary-tab {
			border-bottom-color: Canvas;
		}
		.work-primary-tab.is-selected {
			border-bottom-color: Highlight;
		}
		.work-toggle-tab[aria-pressed='true'] {
			background: Highlight;
			border-color: Highlight;
			color: HighlightText;
		}
	}
</style>
