<!--
  @component
  Unified Work queue shared by `/work` (organization scope) and `/projects/[id]/work` (project scope)
  (work-items-ux §6, §9, §11; feature-requests-ux §2; PRJ-1, WORK-1..10, TSK-1..10, BUG-10..15, RQS-7..15).
  Uses Resource + Loaded (handling loading, unavailable, unsupported, error, ready).
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { SvelteURLSearchParams } from 'svelte/reactivity';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { gutters, inputClass, mobileButton, selectClass } from '$lib/components/classes';
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
		isOpen,
		severityWord,
		type GroupBy
	} from '../../present';
	import type { TaskType, WorkQueue, WorkScope } from '../../types';
	import StaleBanner from '../../components/StaleBanner.svelte';
	import CaptureDialog from '../capture/CaptureDialog.svelte';
	import WorkBoard from './WorkBoard.svelte';
	import WorkList from './WorkList.svelte';
	import NeedsMeList from './NeedsMeList.svelte';

	let {
		organizationId,
		scope,
		projectArchived = false
	}: {
		organizationId: string;
		scope: WorkScope;
		projectArchived?: boolean;
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
	let bugGroupBy = $state<GroupBy>('assigned_team');
	let requestGroupBy = $state<GroupBy>('status');
	let filters = $state<QueueFilters>(defaultFilters());
	let savedExplicit = $state(false);
	let showOverride = $state<ShowView | null>(null);

	let captureOpen = $state(false);
	let captureType = $state<TaskType>('task');

	// Sync Show view with `?view=` on `/work`.
	const urlView = $derived(page.url.searchParams.get('view'));
	const activeShow = $derived.by<ShowView>(() => {
		if (!isOrgScope) return 'mine';
		if (isShowView(urlView)) return urlView;
		if (showOverride) return showOverride;
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

	function setShow(next: ShowView) {
		showOverride = next;
		if (next === 'mine') {
			savedExplicit = false;
			if (filters.saved !== 'all') filters = { ...filters, saved: 'all' };
		}
		const params = new SvelteURLSearchParams(page.url.searchParams);
		params.set('view', next);
		void goto(resolve(`${page.url.pathname}?${params.toString()}` as '/'), {
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

	const bugGroupOptions: GroupBy[] = [
		'assigned_team',
		'severity',
		'priority',
		'status',
		'team',
		'assignee',
		'none'
	];
	const requestGroupOptions: GroupBy[] = ['status', 'assigned_team', 'team', 'assignee', 'none'];
</script>

{#if isOrgScope}
	<PageHeader
		title="Work"
		count={queueRead.data?.rows.length}
		countLabel="items"
		description="Tasks, bugs and requests across projects and teams."
	>
		{#snippet actions()}
			{#if queueRead.data}
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={!!queueRead.data.stale}
					onclick={() => openCapture('request')}>New request</Button
				>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={!!queueRead.data.stale}
					onclick={() => openCapture('bug')}>Report bug</Button
				>
				<Button
					type="button"
					class={mobileButton}
					disabled={!!queueRead.data.stale}
					onclick={() => openCapture('task')}>New task</Button
				>
			{/if}
		{/snippet}
	</PageHeader>
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

		{#if q.stale}
			<StaleBanner
				stale={q.stale}
				retrying={queueRead.reloading}
				onretry={() => queueRead.reload()}
			/>
		{/if}

		<div class="flex min-w-0 flex-col gap-5 {gutters} py-6">
			<!-- Top controls: Type tabs + Project actions / Show menu / Board-List toggle -->
			<div class="flex flex-wrap items-center justify-between gap-3 border-b border-line pb-2">
				<div role="tablist" aria-label="Work" class="flex flex-wrap gap-x-1">
					{#each tabs as t, index (t.id)}
						{@const selected = activeTab === t.id && (!isOrgScope || showMode === 'mine')}
						<button
							id="{uid}-tab-{t.id}"
							type="button"
							role="tab"
							aria-selected={selected}
							aria-controls="{uid}-panel"
							tabindex={activeTab === t.id ? 0 : -1}
							onclick={() => selectTab(t.id)}
							onkeydown={(e) => handleTabKey(e, index)}
							class="px-3 py-2 text-sm/6 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 {selected
								? 'border-b-2 border-brand font-semibold text-fg'
								: 'text-fg-secondary hover:text-fg'}"
						>
							{t.label}
							<span class="ml-1 text-xs/5 font-normal text-fg-muted tabular-nums">
								{q.rows.filter((r) => r.type === t.id).length}
							</span>
						</button>
					{/each}
				</div>

				<div class="flex flex-wrap items-center gap-2">
					{#if isOrgScope}
						<label class="flex items-center gap-2 text-xs/5 text-fg-muted">
							<span>Show</span>
							<select
								aria-label="Show"
								value={showMode}
								onchange={(e) => setShow((e.currentTarget as HTMLSelectElement).value as ShowView)}
								class="w-auto {selectClass}"
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

					{#if (!isOrgScope || (showMode !== 'needs_me' && showMode !== 'by_project' && showMode !== 'by_assignee')) && activeTab === 'task'}
						<div
							role="group"
							aria-label="Layout"
							class="inline-flex rounded-md bg-surface-raised p-0.5 ring-1 ring-line"
						>
							<button
								type="button"
								aria-pressed={layout === 'board'}
								onclick={() => setLayout('board')}
								class="rounded-sm px-2.5 py-1 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-focus max-[639px]:min-h-11 {layout ===
								'board'
									? 'bg-surface-overlay text-fg'
									: 'text-fg-secondary hover:text-fg'}">Board</button
							>
							<button
								type="button"
								aria-pressed={layout === 'list'}
								onclick={() => setLayout('list')}
								class="rounded-sm px-2.5 py-1 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-focus max-[639px]:min-h-11 {layout ===
								'list'
									? 'bg-surface-overlay text-fg'
									: 'text-fg-secondary hover:text-fg'}">List</button
							>
						</div>
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

			<!-- Main panel -->
			<div
				id="{uid}-panel"
				role={tabSelected ? 'tabpanel' : 'region'}
				aria-labelledby={tabSelected ? `${uid}-tab-${activeTab}` : undefined}
				aria-label={tabSelected ? undefined : activeShowLabel}
				class="flex min-w-0 flex-col gap-4"
			>
				{#if isOrgScope && showMode === 'needs_me'}
					<NeedsMeList rows={needsMeItems} sources={q.needsMeSources} />
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
									class="inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 {pressed
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
						<div class="w-full min-[640px]:w-56">
							<label for="{uid}-search" class="sr-only">Search work</label>
							<Input
								id="{uid}-search"
								type="search"
								bind:value={filters.query}
								placeholder="Search title or T- ref…"
								aria-describedby="{uid}-search-scope"
								class={inputClass}
							/>
							<span id="{uid}-search-scope" class="sr-only">
								Searches shared titles, references, teams and projects.
							</span>
						</div>

						<select
							aria-label="Filter by status"
							bind:value={filters.status}
							class="w-auto {selectClass}"
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
								class="w-auto {selectClass}"
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
							class="w-auto {selectClass}"
						>
							<option value="all">All priorities</option>
							{#each PRIORITIES as p (p)}
								<option value={p}>{PRIORITY_WORD[p]}</option>
							{/each}
						</select>

						{#if isOrgScope}
							<select
								aria-label="Filter by project"
								bind:value={filters.projectId}
								class="w-auto {selectClass}"
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
							class="w-auto {selectClass}"
						>
							<option value="">Accountable team</option>
							{#each uniqueTeams as [id, name] (id)}
								<option value={id}>{name}</option>
							{/each}
						</select>

						<select
							aria-label="Filter by assigned team"
							bind:value={filters.assignedTeamId}
							class="w-auto {selectClass}"
						>
							<option value="">Assigned team</option>
							<option value="none">Unassigned team</option>
							{#each uniqueTeams as [id, name] (id)}
								<option value={id}>{name}</option>
							{/each}
						</select>

						<select
							aria-label="Filter by assignee"
							bind:value={filters.assigneeId}
							class="w-auto {selectClass}"
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
							class="w-auto {selectClass}"
						>
							<option value="">Reporter</option>
							{#each uniquePeople as [id, name] (id)}
								<option value={id}>{name}</option>
							{/each}
						</select>

						{#if activeTab === 'bug' && (!isOrgScope || showMode === 'mine')}
							<select
								aria-label="Group bugs by"
								bind:value={bugGroupBy}
								class="w-auto {selectClass}"
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
								class="w-auto {selectClass}"
								data-testid="group-by-select"
							>
								{#each requestGroupOptions as g (g)}
									<option value={g}>Group: {GROUP_WORD[g]}</option>
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
							<WorkList rows={filtered} tab="all" groupBy="project" showProject={false} />
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
							<WorkList rows={filtered} tab="all" groupBy="assignee" showProject={true} />
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
					{:else if displayRows.length === 0}
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
							rows={displayRows}
							showProject={isOrgScope}
							stale={isStale}
							onchanged={() => queueRead.reload()}
						/>
					{:else}
						<WorkList
							rows={displayRows}
							tab={activeTab}
							groupBy={activeTab === 'bug'
								? bugGroupBy
								: activeTab === 'request'
									? requestGroupBy
									: 'none'}
							showProject={isOrgScope}
						/>
					{/if}
				{/if}
			</div>
		</div>
	{/snippet}
</Loaded>

<CaptureDialog
	bind:open={captureOpen}
	{organizationId}
	initialType={captureType}
	{projectId}
	oncreated={() => queueRead.reload()}
	onadded={() => queueRead.reload()}
/>
