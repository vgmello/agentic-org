<!--
  @component
  Unified Work queue shared by `/work` (organization scope) and `/projects/[id]/work` (project scope)
  (work-items-ux §6, §9, §11; feature-requests-ux §2; PRJ-1, WORK-1..10, TSK-1..10, BUG-10..15, RQS-7..15).
-->
<script lang="ts">
	import { tick, type Snippet } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { SvelteSet, SvelteURLSearchParams } from 'svelte/reactivity';
	import { Tooltip } from 'bits-ui';
	import { toast } from 'svelte-sonner';
	import Loaded from '$lib/components/Loaded.svelte';
	import WorkReadingPanel from '$lib/components/WorkReadingPanel.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { gutters } from '$lib/components/classes';
	import { useShellCommand } from '$lib/platform/workspace/shell-command';
	import { useShellData } from '$lib/platform/workspace/shell-data-context';
	import { useScopedSearchContext } from '$lib/platform/workspace/scoped-search.svelte';
	import { singleKeyShortcutsOn } from '$lib/platform/workspace/shortcuts';
	import { uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';
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
	import type { TaskRow, TaskType, WorkQueue, WorkScope } from '../../types';
	import StaleBanner from '../../components/StaleBanner.svelte';
	import CaptureDialog from '../capture/CaptureDialog.svelte';
	import WorkDetailPage from '../detail/WorkDetailPage.svelte';
	import { buildActiveFilterChips, type ActiveFilterChip } from './active-chips';
	import {
		EMPTY_SAVED_COUNTS,
		attachWorkSearchShortcuts,
		buildSubgroupCandidates,
		extractUniqueFacets,
		focusWorkSearchInput,
		groupOptionsForTab,
		readingPriority,
		readingStatusTone,
		resolveBoardBaseRows,
		restoreInlineDetailFocus
	} from './work-queue-helpers';
	import WorkProjectLayoutControls from './WorkProjectLayoutControls.svelte';
	import WorkQueueBody from './WorkQueueBody.svelte';
	import WorkQueueHeader from './WorkQueueHeader.svelte';

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

	let activeTab = $derived<TaskType>(isOrgScope ? readStoredTab(organizationId) : 'task');
	let layout = $derived<LayoutMode>(readStoredLayout(scopeKey));
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
		restoreInlineDetailFocus({
			closedRef,
			trigger,
			savedScrollTop: savedQueueScrollTop,
			stillOpen: () => Boolean(openRowRef)
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

	function focusSearchAfterShowSwitch() {
		const run = async () => {
			if (isOrgScope && activeShow === 'needs_me' && queueRead.data) {
				await setShow('mine');
			}
			await tick();
			focusWorkSearchInput(uid);
		};
		void run();
	}

	function workSearchShortcut() {
		return attachWorkSearchShortcuts({
			uid,
			shortcutsEnabled: () => shortcutsEnabled,
			hasOpenRow: () => Boolean(activeOpenRow),
			needsShowSwitch: () => isOrgScope && activeShow === 'needs_me' && Boolean(queueRead.data),
			usesHeaderScopedSearch: () => usesHeaderScopedSearch,
			onCloseDetail: closeInlineDetail,
			onFocusSearchAfterSwitch: focusSearchAfterShowSwitch
		});
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
		if (isOrgScope && activeShow !== 'mine') void setShow('mine');
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

	function openCapture(kind: TaskType) {
		captureType = kind;
		captureOpen = true;
	}

	function clearFilters() {
		filters = defaultFilters();
		savedExplicit = false;
		if (isOrgScope && (urlProject || urlAssignee)) syncFilterUrl('', '');
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

	const facets = $derived(extractUniqueFacets(queueRead.data?.rows ?? []));
	const archivedIds = $derived.by(() => {
		const ids = new SvelteSet<string>();
		if (uiPrefs.archiveAfterDays <= 0) return ids;
		for (const r of queueRead.data?.rows ?? []) {
			if (isArchivedTaskRow(r, uiPrefs.archiveAfterDays)) ids.add(r.id);
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
			: EMPTY_SAVED_COUNTS
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
	const groupOptionsState = $derived(groupOptionsForTab(activeTab, isOrgScope));
	const showGroupMenuState = $derived(
		isListSurfaceState && (!isOrgScope || showModeState === 'mine')
	);
	const subgroupCandidatesState = $derived(
		buildSubgroupCandidates(activeTab, isOrgScope, showModeState, primaryGroupState)
	);
	const effectiveSubgroupState = $derived<GroupBy>(
		primaryGroupState === 'none' || !subgroupCandidatesState.includes(subgroupBy)
			? 'none'
			: subgroupBy
	);
	const activeChips = $derived(
		buildActiveFilterChips({
			filters,
			activeTab,
			projects: facets.uniqueProjects,
			teams: facets.uniqueTeams,
			people: facets.uniquePeople
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
		<WorkProjectLayoutControls
			{layout}
			{boardMode}
			onLayoutChange={setLayout}
			onBoardModeChange={(next) => (boardMode = next)}
		/>
	{/if}
{/snippet}

{#snippet queueHeader(staleFlag: boolean)}
	<WorkQueueHeader
		{uid}
		{isOrgScope}
		hasData={Boolean(queueRead.data)}
		{openItemCount}
		{needsMeCount}
		hasShellCommand={Boolean(shellCommand)}
		{shortcutsEnabled}
		{activeTab}
		showMode={showModeState}
		{visibleQueueRows}
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
		uniqueProjects={facets.uniqueProjects}
		uniqueTeams={facets.uniqueTeams}
		uniquePeople={facets.uniquePeople}
		showLayoutToggle={showLayoutToggleState}
		canCreate={isOrgScope || !projectArchived}
		createDisabled={staleFlag}
		onSelectTab={selectTab}
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
			{@render queueHeader(Boolean(queueRead.data?.stale))}
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
				{@const boardBase = resolveBoardBaseRows({
					allRows: q.rows,
					fallbackBase: baseRows,
					archivedIds,
					showArchived,
					scopeMine: scopeMineState,
					mineFilter: (r) => mineRows(r, q.viewer)
				})}
				{@const boardRows = filterRows(
					boardBase,
					q.viewer,
					boardMode === 'statuses' ? 'all' : activeTab,
					filters
				)}
				{@const showNoEmployeeNotice =
					!q.viewer.employee &&
					(filters.saved === 'owned_by_my_team' ||
						filters.saved === 'assigned_to_my_team' ||
						filters.saved === 'assigned_to_me' ||
						filters.saved === 'reported_by_me' ||
						scopeMineState)}

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
						{@render queueHeader(isStale)}
					{/if}

					<WorkQueueBody
						{uid}
						{organizationId}
						queue={q}
						{isOrgScope}
						{isStale}
						{projectArchived}
						{tabSelected}
						{activeShowLabel}
						showMode={showModeState}
						{activeTab}
						{layout}
						{boardMode}
						primaryGroup={primaryGroupState}
						effectiveSubgroup={effectiveSubgroupState}
						{sortBy}
						{sortDir}
						{showDividers}
						{showArchived}
						{archivedIds}
						{openRowRef}
						{usesHeaderScopedSearch}
						{shortcutsEnabled}
						{showNoEmployeeNotice}
						{activeChips}
						{filters}
						scopeMine={scopeMineState}
						{needsMeItems}
						{typeTotal}
						{filtered}
						{boardRows}
						bind:query={filters.query}
						onOpenDetail={openInlineDetail}
						onCreate={openCapture}
						onClearFilters={clearFilters}
						onShowAll={() => selectSaved('all')}
						onShowByProject={() => void setShow('by_project')}
						onHideArchived={() => setShowArchived(false)}
						onRemoveChip={removeFilterChip}
						onChanged={reloadQueue}
					/>
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
</style>
