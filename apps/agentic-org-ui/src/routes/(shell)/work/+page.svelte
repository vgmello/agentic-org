<script lang="ts">
	import { onDestroy, tick, untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { navigating, page } from '$app/state';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import Columns3 from '@lucide/svelte/icons/columns-3';
	import FileCheck2 from '@lucide/svelte/icons/file-check-2';
	import List from '@lucide/svelte/icons/list';
	import ListFilter from '@lucide/svelte/icons/list-filter';
	import PauseCircle from '@lucide/svelte/icons/pause-circle';
	import Plus from '@lucide/svelte/icons/plus';
	import Search from '@lucide/svelte/icons/search';
	import SlidersHorizontal from '@lucide/svelte/icons/sliders-horizontal';
	import X from '@lucide/svelte/icons/x';
	import { toast } from 'svelte-sonner';
	import { owner } from '$lib/org/model';
	import BugSheet from '$lib/components/BugSheet.svelte';
	import NewRequestDialog from '$lib/components/NewRequestDialog.svelte';
	import NewTaskDialog from '$lib/components/NewTaskDialog.svelte';
	import RequestSheet from '$lib/components/RequestSheet.svelte';
	import RequestsPanel from '$lib/components/RequestsPanel.svelte';
	import TaskSheet from '$lib/components/TaskSheet.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { toAppError } from '$lib/office/api';
	import { isEditableTarget, singleKeyShortcutsOn } from '$lib/platform/shell/shortcuts';
	import type { WorkItemSummary } from '$lib/proto';
	import { sender, workApi } from '$lib/work';
	import { bugRef, severityWord } from '$lib/work/bugs';
	import {
		assigneeInitials,
		needsOwnerAttention,
		priorityMeter,
		projectDotTone,
		workStatusLabel,
		workStatusTone
	} from '$lib/work/compact';
	import { nameOf } from '$lib/work/people';
	import { requestRef, requestsNeedingYou } from '$lib/work/requests-view';
	import { countOf, defaultFilter, needsYou, savedFilters, type WorkTab } from '$lib/work/saved';
	import { priorityWord, taskRef } from '$lib/work/tasks';
	import { severityTone, type WorkTone } from '$lib/work/tones';
	import {
		deriveBoardColumns,
		deriveWorkScopeOptions,
		filterWorkItems,
		groupWorkItems,
		hasWorkScopeFilters,
		openWorkCount,
		WORK_DIVIDERS_STORAGE_KEY,
		WORK_GROUPING_STORAGE_KEY,
		WORK_LAYOUT_STORAGE_KEY,
		workKindCounts,
		type WorkGrouping,
		type WorkLayout,
		type WorkScopeFilter
	} from '$lib/work/work-toolbar';

	/** Work across every project (work-items-ux §11): Tasks, Bugs, and Requests with unified Inbox-parity controls. */
	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const snapshot = $derived(data.snapshot);
	const me = $derived(owner(snapshot)?.id ?? 'owner');
	const send = $derived(sender(organizationId));
	/** The open requests that read like these words, from the office (offered while typing). */
	const find = (text: string) =>
		workApi.similar(organizationId, { text, limit: 2 }).then((r) => r.items);
	const projectKey = (id: string | undefined) =>
		(snapshot.projects ?? []).find((p) => p.id === id)?.key || undefined;
	const projectName = (id: string | undefined) =>
		(snapshot.projects ?? []).find((p) => p.id === id)?.name ?? 'No project';
	const itemRef = (item: WorkItemSummary) =>
		item.kind === 'bug'
			? bugRef(item, projectKey(item.project_id))
			: item.kind === 'request'
				? requestRef(item)
				: taskRef(item, projectKey(item.project_id));

	let items = $state.raw<WorkItemSummary[] | undefined>();
	let loadError = $state<string | undefined>();
	let tab = $state<WorkTab>('tasks');
	let filterId = $state('all');
	let chosen = false;
	let searchQuery = $state('');
	let searchInputEl = $state<HTMLInputElement | null>(null);
	let taskId = $state<string | undefined>();
	let bugId = $state<string | undefined>();
	let taskOpen = $state(false);
	let bugOpen = $state(false);
	// What the queue's ages count from: when the list was last loaded.
	let now = $state(new Date().toISOString());
	let requestId = $state<string | undefined>();
	let requestStart = $state<'route' | undefined>();
	let requestOpen = $state(false);
	let newRequestOpen = $state(false);
	let newTaskOpen = $state(false);
	const requestsOn = workApi.requests;
	const defaultTaskProject = $derived((snapshot.projects ?? []).find((p) => !p.archived));
	const shortcutsEnabled = $derived(singleKeyShortcutsOn());

	// URL-synced Project & Assignee scope filters (writable derived avoids $effect state writes).
	let scope = $derived<WorkScopeFilter>({
		project: page.url.searchParams.get('project') || undefined,
		assignee: page.url.searchParams.get('assignee') || undefined
	});

	// Local display preferences (persisted cleanly across sessions).
	function readDividers(): boolean {
		if (typeof window === 'undefined') return false;
		try {
			return window.localStorage.getItem(WORK_DIVIDERS_STORAGE_KEY) === '1';
		} catch {
			return false;
		}
	}
	function readLayout(): WorkLayout {
		if (typeof window === 'undefined') return 'list';
		try {
			return window.localStorage.getItem(WORK_LAYOUT_STORAGE_KEY) === 'board' ? 'board' : 'list';
		} catch {
			return 'list';
		}
	}
	function readGrouping(): WorkGrouping {
		if (typeof window === 'undefined') return 'status';
		try {
			return window.localStorage.getItem(WORK_GROUPING_STORAGE_KEY) === 'project'
				? 'project'
				: 'status';
		} catch {
			return 'status';
		}
	}

	let showDividers = $state(readDividers());
	let layout = $state<WorkLayout>(readLayout());
	let grouping = $state<WorkGrouping>(readGrouping());

	function toggleDividers(next: boolean) {
		showDividers = next;
		try {
			window.localStorage.setItem(WORK_DIVIDERS_STORAGE_KEY, next ? '1' : '0');
		} catch {
			// kept for this session only
		}
	}
	function setLayout(next: WorkLayout) {
		layout = next;
		try {
			window.localStorage.setItem(WORK_LAYOUT_STORAGE_KEY, next);
		} catch {
			// kept for this session only
		}
	}
	function setGrouping(next: WorkGrouping) {
		grouping = next;
		try {
			window.localStorage.setItem(WORK_GROUPING_STORAGE_KEY, next);
		} catch {
			// kept for this session only
		}
	}

	// Work opens on the tab it was left on (per organization), as the queue's filters are remembered.
	const TAB_KEY = (id: string) => `agentic-org.work.tab.${id}`;
	function rememberedTab(orgId: string): WorkTab {
		try {
			const saved = window.localStorage.getItem(TAB_KEY(orgId));
			if (saved === 'tasks' || saved === 'bugs' || (saved === 'requests' && workApi.requests))
				return saved;
		} catch {
			// unreadable storage: open on Tasks
		}
		return 'tasks';
	}

	let loadSeq = 0;
	let destroyed = false;

	async function load(targetOrgId = organizationId, isCancelled: () => boolean = () => false) {
		const seq = ++loadSeq;
		try {
			const got = (await workApi.snapshot(targetOrgId)).items;
			if (destroyed || isCancelled() || seq !== loadSeq) return;
			items = got;
			now = new Date().toISOString();
			loadError = undefined;
			// Open on the default filter of the live tab until the owner picks one.
			if (!chosen) filterId = defaultFilter(tab, got, me);
		} catch (e) {
			if (destroyed || isCancelled() || seq !== loadSeq) return;
			if (items === undefined) loadError = toAppError(e).message;
			else toast.error(toAppError(e).message);
		}
	}

	$effect(() => {
		const id = organizationId;
		let cancelled = false;
		untrack(() => {
			const nextTab = page.url.searchParams.get('request') ? tab : rememberedTab(id);
			tab = nextTab;
			chosen = false;
			items = undefined;
			loadError = undefined;
			taskOpen = false;
			bugOpen = false;
			requestOpen = false;
			newRequestOpen = false;
			newTaskOpen = false;
			taskId = undefined;
			bugId = undefined;
			requestId = undefined;
			requestStart = undefined;
			void load(id, () => cancelled);
		});
		return () => {
			cancelled = true;
		};
	});

	onDestroy(() => {
		destroyed = true;
	});

	// Composes overlapping URL scope updates cleanly while preserving other query/hash parameters.
	const workPathname = resolve('/work');
	const isLeavingWork = () =>
		page.url.pathname !== workPathname ||
		Boolean(navigating.to && navigating.to.url.pathname !== workPathname);
	let pendingUrl: URL | undefined;
	let pendingBaseHref = '';
	let navSeq = 0;

	function syncScopeUrl(next: { project?: string | null; assignee?: string | null }) {
		if (destroyed || isLeavingWork()) return;
		const liveHref = page.url.href;
		const hasPending = Boolean(pendingUrl && liveHref === pendingBaseHref);
		const url = hasPending && pendingUrl ? new URL(pendingUrl) : new URL(page.url);
		pendingBaseHref = liveHref;

		if ('project' in next) {
			if (next.project) url.searchParams.set('project', next.project);
			else url.searchParams.delete('project');
		}
		if ('assignee' in next) {
			if (next.assignee) url.searchParams.set('assignee', next.assignee);
			else url.searchParams.delete('assignee');
		}

		const shouldNavigate = hasPending ? pendingUrl?.href !== url.href : url.href !== liveHref;
		if (!shouldNavigate) return;
		const seq = ++navSeq;
		pendingUrl = new URL(url);
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- cloned page.url preserves deployment base path and existing query/hash params
		void goto(url, {
			replaceState: true,
			noScroll: true,
			keepFocus: true
		}).finally(() => {
			if (seq === navSeq) pendingUrl = undefined;
		});
	}

	function setScope(nextScope: WorkScopeFilter) {
		scope = nextScope;
		syncScopeUrl({
			project: nextScope.project ?? null,
			assignee: nextScope.assignee ?? null
		});
	}

	function resetScopeFilters() {
		setScope({});
	}

	function clearAllFilters() {
		searchQuery = '';
		setScope({});
	}

	function openNewTask() {
		if (defaultTaskProject) {
			newTaskOpen = true;
		} else {
			toast('Create a project first to add a task.');
		}
	}

	// Home's "New task" CTA links to /work?new=1: open the task composer and strip the parameter so reload doesn't reopen it.
	const wantsNewTask = $derived(page.url.searchParams.get('new') === '1');
	$effect(() => {
		if (!wantsNewTask) return;
		if (defaultTaskProject) {
			tab = 'tasks';
			newTaskOpen = true;
		} else {
			toast('Create a project first to add a task.');
		}
		const nextUrl = new URL(page.url);
		nextUrl.searchParams.delete('new');
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- preserving current already-resolved URL + hash
		void goto(nextUrl, {
			replaceState: true,
			noScroll: true,
			keepFocus: true
		});
	});

	// A link to a request ("From REQ-12" on a task) opens its page here, whether this page was just opened or already open.
	const wanted = $derived(page.url.searchParams.get('request'));
	$effect(() => {
		if (!wanted || items === undefined) return;
		if (workApi.requests && items.some((i) => i.id === wanted && i.kind === 'request')) {
			tab = 'requests';
			requestId = wanted;
			requestStart = undefined;
			requestOpen = true;
		} else if (workApi.requests) toast('That request is no longer here.');
		// Only the link's own parameter goes; pathname (already base-prefixed) and hash stay intact.
		const nextUrl = new URL(page.url);
		nextUrl.searchParams.delete('request');
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- preserving current already-resolved URL + hash
		void goto(nextUrl, {
			replaceState: true,
			noScroll: true,
			keepFocus: true
		});
	});

	function pick(next: WorkTab) {
		tab = next;
		try {
			window.localStorage.setItem(TAB_KEY(organizationId), next);
		} catch {
			// kept for this visit only
		}
		chosen = false;
		filterId = defaultFilter(next, items ?? [], me);
	}

	const enabledTabs = $derived<WorkTab[]>(
		requestsOn ? ['tasks', 'bugs', 'requests'] : ['tasks', 'bugs']
	);

	function handleTabKeydown(event: KeyboardEvent, current: WorkTab) {
		const idx = enabledTabs.indexOf(current);
		if (idx < 0) return;
		const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
		if (!step && event.key !== 'Home' && event.key !== 'End') return;
		event.preventDefault();
		const nextIdx =
			event.key === 'Home'
				? 0
				: event.key === 'End'
					? enabledTabs.length - 1
					: (idx + step + enabledTabs.length) % enabledTabs.length;
		const nextTab = enabledTabs[nextIdx];
		pick(nextTab);
		void tick().then(() => document.getElementById(`work-tab-${nextTab}`)?.focus());
	}

	function handleSearchKeydown(event: KeyboardEvent) {
		if (event.key !== 'Escape') return;
		event.preventDefault();
		if (searchQuery) {
			searchQuery = '';
		} else {
			searchInputEl?.blur();
		}
	}

	// Capture unmodified `/` on the Work page to focus inline search without opening global CmdK.
	function workShortcuts() {
		const listener = (event: KeyboardEvent) => {
			if (
				event.defaultPrevented ||
				event.isComposing ||
				event.keyCode === 229 ||
				event.metaKey ||
				event.ctrlKey ||
				event.altKey ||
				event.shiftKey ||
				event.key !== '/'
			) {
				return;
			}
			if (
				document.querySelector(
					'[role="dialog"], [role="menu"], [role="alertdialog"], [role="listbox"]'
				)
			) {
				return;
			}
			if (isEditableTarget(event.target)) return;
			event.stopPropagation();
			if (!shortcutsEnabled || event.repeat) return;
			event.preventDefault();
			searchInputEl?.focus();
		};
		window.addEventListener('keydown', listener, { capture: true });
		return () => window.removeEventListener('keydown', listener, { capture: true });
	}

	function pillToneClass(tone: WorkTone): string {
		if (tone === 'ok') return 't-green';
		if (tone === 'warn') return 't-amber';
		if (tone === 'bad') return 't-red';
		if (tone === 'accent') return 'work-pill-accent';
		return 'bg-surface-raised text-fg-secondary ring-1 ring-line ring-inset';
	}

	function openItem(item: WorkItemSummary) {
		if (item.kind === 'bug') {
			bugId = item.id;
			bugOpen = true;
		} else {
			taskId = item.id;
			taskOpen = true;
		}
	}

	const filters = $derived(savedFilters(tab, me));
	const active = $derived(
		filters.find((f) => f.id === filterId) ??
			filters[filters.length - 1] ?? { id: '', label: '', test: () => false }
	);
	const scopeOpts = $derived(deriveWorkScopeOptions(items ?? [], snapshot));
	const scopedPool = $derived(
		filterWorkItems(items ?? [], {
			filterTest: () => true,
			scope,
			query: searchQuery,
			getRef: itemRef,
			getProjectName: projectName,
			getAssigneeName: (id) => nameOf(snapshot, id)
		})
	);
	const shown = $derived(
		filterWorkItems(items ?? [], {
			filterTest: active.test,
			scope,
			query: searchQuery,
			getRef: itemRef,
			getProjectName: projectName,
			getAssigneeName: (id) => nameOf(snapshot, id)
		})
	);
	const groups = $derived(
		tab === 'requests' ? [] : groupWorkItems(shown, tab, grouping, projectName)
	);
	const boardColumns = $derived(tab === 'requests' ? [] : deriveBoardColumns(shown, tab));
	const openCount = $derived(openWorkCount(items ?? []));
	const kindCounts = $derived(workKindCounts(items ?? []));
	const waiting = $derived(needsYou(items ?? [], me));
	const requestsWaiting = $derived(requestsNeedingYou(items ?? []));
	const isScopeFiltered = $derived(hasWorkScopeFilters(scope));
	const displayActive = $derived(grouping !== 'status' || showDividers);
	const hasActiveFilters = $derived(Boolean(searchQuery.trim() || isScopeFiltered));
	const activeItemId = $derived(taskOpen ? taskId : bugOpen ? bugId : undefined);
	const bugsOf = (projectId: string | undefined) =>
		(items ?? []).filter((i) => i.kind === 'bug' && i.project_id === projectId);
	const areasOf = (projectId: string | undefined) =>
		(snapshot.areas ?? [])
			.filter((a) => a.project_id === projectId && !a.deleted)
			.map((a) => ({ id: a.id, name: a.name, general: a.general }));
	const bugProject = $derived((items ?? []).find((i) => i.id === bugId)?.project_id);
	// WAI-ARIA Tabs Pattern: make the panel tabbable whenever it begins with non-focusable copy or has no interactive controls.
	const panelNeedsFocus = $derived(
		Boolean(loadError) ||
			items === undefined ||
			tab === 'requests' ||
			(snapshot.projects ?? []).length === 0
	);
</script>

<svelte:head><title>Work · Agentic Org</title></svelte:head>

<div class="work-page flex w-full flex-col">
	<section
		{@attach workShortcuts}
		aria-label="Work workspace"
		data-work-dividers={showDividers ? '' : undefined}
		class="work-workspace flex min-h-0 w-full flex-col"
	>
		<!-- Unified Inbox-parity header & controls (poc/operations.css .work-header.queue-header) -->
		<header class="work-header queue-header">
			<div class="queue-headline">
				<div class="ph-t">
					<h1 id="shell-page-title" tabindex="-1" class="work-h1">Work</h1>
					<span role="status" class="queue-count">
						<i class={[waiting === 0 && 'is-ok']} aria-hidden="true"></i>
						{#if items !== undefined}
							<span class="tabular-nums">{openCount} open</span>
							<span aria-hidden="true">·</span>
						{/if}
						<span class="tabular-nums" data-testid="needs-you">
							{waiting === 0
								? 'Nothing needs you.'
								: `${waiting} ${waiting === 1 ? 'item needs' : 'items need'} you`}
						</span>
					</span>
				</div>

				<div class="queue-search-wrap">
					<div class="work-search">
						<Search class="work-search-ico size-4 shrink-0" aria-hidden="true" />
						<label class="sr-only" for="work-search-input">Search work</label>
						<input
							bind:this={searchInputEl}
							value={searchQuery}
							oninput={(e) => (searchQuery = e.currentTarget.value)}
							onkeydown={handleSearchKeydown}
							id="work-search-input"
							type="search"
							autocomplete="off"
							placeholder="Search work…"
							class="work-search-input"
						/>
						{#if searchQuery}
							<button
								type="button"
								onclick={() => {
									searchQuery = '';
									searchInputEl?.focus();
								}}
								aria-label="Clear search"
								class="work-clear-btn inline-flex min-h-9 min-w-9 items-center justify-center [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11"
							>
								<X class="size-3.5" aria-hidden="true" />
							</button>
						{:else if shortcutsEnabled}
							<kbd class="work-search-kbd" aria-hidden="true">/</kbd>
						{/if}
					</div>
				</div>
			</div>

			<div class="work-toolbar queue-toolbar">
				<div role="tablist" aria-label="Work" class="queue-tabs">
					{#each [['tasks', 'Tasks'], ['bugs', 'Bugs']] as const as [id, name] (id)}
						<button
							id="work-tab-{id}"
							type="button"
							role="tab"
							aria-selected={tab === id}
							aria-controls="work-panel"
							tabindex={tab === id ? 0 : -1}
							onclick={() => pick(id)}
							onkeydown={(e) => handleTabKeydown(e, id)}
							class="queue-tab"
						>
							<span>{name}</span>
							{#if items !== undefined}
								<span class="queue-tab-count">
									{id === 'tasks' ? kindCounts.tasks : kindCounts.bugs}
								</span>
							{/if}
						</button>
					{/each}
					{#each ['Debt'] as name (name)}
						<span
							role="tab"
							aria-selected="false"
							aria-disabled="true"
							title="Available in a later release"
							class="queue-tab is-disabled">{name}</span
						>
					{/each}
					{#if requestsOn}
						<button
							id="work-tab-requests"
							type="button"
							role="tab"
							aria-selected={tab === 'requests'}
							aria-controls="work-panel"
							tabindex={tab === 'requests' ? 0 : -1}
							onclick={() => pick('requests')}
							onkeydown={(e) => handleTabKeydown(e, 'requests')}
							class="queue-tab"
						>
							<span>Requests</span>
							{#if requestsWaiting > 0}
								<span
									class="t-amber inline-flex items-center rounded-full px-1.5 py-0.5 text-[11px]/4 font-semibold tabular-nums"
									data-testid="requests-badge">{requestsWaiting}</span
								>
							{:else if items !== undefined}
								<span class="queue-tab-count">{kindCounts.requests}</span>
							{/if}
						</button>
					{:else}
						<span
							role="tab"
							aria-selected="false"
							aria-disabled="true"
							title="Available in a later release"
							class="queue-tab is-disabled">Requests</span
						>
					{/if}
				</div>

				{#if tab !== 'requests'}
					<div class="work-layout-toggle" role="group" aria-label="Work layout">
						<button
							type="button"
							class="queue-tab"
							aria-pressed={layout === 'list'}
							aria-controls="work-results"
							onclick={() => setLayout('list')}
						>
							<List class="size-4 shrink-0" aria-hidden="true" />
							<span>List</span>
						</button>
						<button
							type="button"
							class="queue-tab"
							aria-pressed={layout === 'board'}
							aria-controls="work-results"
							onclick={() => setLayout('board')}
						>
							<Columns3 class="size-4 shrink-0" aria-hidden="true" />
							<span>Board</span>
						</button>
					</div>

					<div class="queue-controls" role="group" aria-label="Work view controls">
						<DropdownMenu.Root>
							<DropdownMenu.Trigger>
								{#snippet child({ props })}
									<button
										{...props}
										type="button"
										aria-label="Filter work"
										title="Filter work"
										data-active={isScopeFiltered ? '' : undefined}
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
								<DropdownMenu.Label>Project</DropdownMenu.Label>
								<DropdownMenu.RadioGroup
									value={scope.project ?? ''}
									onValueChange={(val) => setScope({ ...scope, project: val || undefined })}
								>
									<DropdownMenu.RadioItem
										value=""
										closeOnSelect={true}
										class="min-h-9 [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
									>
										All projects
									</DropdownMenu.RadioItem>
									{#each scopeOpts.projects as proj (proj.id)}
										<DropdownMenu.RadioItem
											value={proj.id}
											closeOnSelect={true}
											class="min-h-9 [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
										>
											{proj.name}
										</DropdownMenu.RadioItem>
									{/each}
								</DropdownMenu.RadioGroup>
								<DropdownMenu.Separator />
								<DropdownMenu.Label>Assignee</DropdownMenu.Label>
								<DropdownMenu.RadioGroup
									value={scope.assignee ?? ''}
									onValueChange={(val) => setScope({ ...scope, assignee: val || undefined })}
								>
									<DropdownMenu.RadioItem
										value=""
										closeOnSelect={true}
										class="min-h-9 [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
									>
										All assignees
									</DropdownMenu.RadioItem>
									{#each scopeOpts.assignees as person (person.id)}
										<DropdownMenu.RadioItem
											value={person.id}
											closeOnSelect={true}
											class="min-h-9 [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
										>
											{person.name}
										</DropdownMenu.RadioItem>
									{/each}
									<DropdownMenu.RadioItem
										value="unassigned"
										closeOnSelect={true}
										class="min-h-9 [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
									>
										Unassigned
									</DropdownMenu.RadioItem>
								</DropdownMenu.RadioGroup>
								{#if isScopeFiltered}
									<DropdownMenu.Separator />
									<DropdownMenu.Group>
										<DropdownMenu.Item
											onSelect={resetScopeFilters}
											class="min-h-9 [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
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
										aria-label="Work display"
										title="Work display"
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
									value={grouping}
									onValueChange={(val) => {
										if (val === 'status' || val === 'project') setGrouping(val);
									}}
								>
									<DropdownMenu.RadioItem
										value="status"
										closeOnSelect={true}
										class="min-h-9 [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
									>
										By status
									</DropdownMenu.RadioItem>
									<DropdownMenu.RadioItem
										value="project"
										closeOnSelect={true}
										class="min-h-9 [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
									>
										By project
									</DropdownMenu.RadioItem>
								</DropdownMenu.RadioGroup>
								<DropdownMenu.Separator />
								<DropdownMenu.CheckboxItem
									checked={showDividers}
									onCheckedChange={(checked) => toggleDividers(checked === true)}
									class="min-h-9 [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
								>
									Row dividers
								</DropdownMenu.CheckboxItem>
							</DropdownMenu.Content>
						</DropdownMenu.Root>
					</div>
				{/if}

				{#if tab === 'requests' && requestsOn}
					<button
						type="button"
						id="work-new-request"
						data-testid="new-request"
						class="work-cta work-toolbar-cta"
						onclick={() => (newRequestOpen = true)}
					>
						<Plus class="size-4 shrink-0" aria-hidden="true" />
						<span>New request</span>
					</button>
				{:else}
					<button
						type="button"
						id="work-new-task"
						class="work-cta work-toolbar-cta"
						onclick={openNewTask}
					>
						<Plus class="size-4 shrink-0" aria-hidden="true" />
						<span>New task</span>
					</button>
				{/if}
			</div>
		</header>

		<div
			id="work-panel"
			role="tabpanel"
			aria-labelledby="work-tab-{tab}"
			tabindex={panelNeedsFocus ? 0 : -1}
			class="work-body focus-visible:rounded-lg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
		>
			{#if loadError}
				<div class="exec-card flex flex-col items-center gap-3 px-6 py-10 text-center" role="alert">
					<p class="text-sm/6 font-medium text-danger-text">
						{tab === 'requests' ? "Can't load requests." : loadError}
					</p>
					<Button
						type="button"
						variant="outline"
						class="min-h-9 [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
						onclick={() => void load()}
					>
						{tab === 'requests' ? 'Retry' : 'Try again'}
					</Button>
				</div>
			{:else if items === undefined}
				{#if tab === 'requests'}
					<ul
						class="exec-card divide-y divide-line-subtle overflow-hidden"
						aria-busy="true"
						aria-label="Loading requests"
						data-testid="requests-loading"
					>
						{#each [0, 1, 2] as n (n)}
							<li class="flex items-center gap-x-3 px-4 py-3.5">
								<span class="h-4 w-14 rounded bg-surface-overlay"></span>
								<span class="h-4 flex-1 rounded bg-surface-overlay"></span>
								<span class="h-4 w-24 rounded bg-surface-overlay"></span>
							</li>
						{/each}
					</ul>
				{:else}
					<div
						class="exec-card divide-y divide-line-subtle overflow-hidden"
						role="status"
						aria-busy="true"
					>
						<p class="px-5 py-4 text-sm/6 text-fg-muted">Loading work…</p>
					</div>
				{/if}
			{:else if tab === 'requests'}
				{#key organizationId}<RequestsPanel
						{items}
						{snapshot}
						{me}
						{now}
						{organizationId}
						externalQuery={searchQuery}
						onClearExternalQuery={() => (searchQuery = '')}
						hideNewButton={true}
						onopen={(id, start) => ((requestId = id), (requestStart = start), (requestOpen = true))}
						onnew={() => (newRequestOpen = true)}
					/>{/key}
			{:else if (snapshot.projects ?? []).length === 0}
				<div
					class="exec-card flex flex-col items-center gap-1.5 px-6 py-14 text-center"
					data-testid="no-projects"
				>
					<p class="text-sm/6 font-semibold text-fg">
						No projects yet. Work items live in projects.
					</p>
				</div>
			{:else}
				<div class="work-presets-bar">
					<nav aria-label="Saved filters" class="work-presets queue-tabs">
						{#each filters as f (f.id)}
							{@const n = countOf(scopedPool, f)}
							<button
								type="button"
								aria-pressed={active.id === f.id}
								data-filter={f.id}
								onclick={() => ((chosen = true), (filterId = f.id))}
								class="work-preset"
							>
								<span>{f.label}</span>
								<span class="work-preset-count tabular-nums">{n}</span>
							</button>
						{/each}
					</nav>
					{#if hasActiveFilters}
						<div class="work-active-summary">
							<span class="tabular-nums">{shown.length} shown</span>
							<button type="button" class="work-reset-link" onclick={clearAllFilters}>
								Reset filters
							</button>
						</div>
					{/if}
				</div>

				<div id="work-results" class="work-results">
					{#if shown.length === 0}
						<div
							class="exec-card flex flex-col items-center gap-2.5 px-6 py-14 text-center"
							data-testid="no-match"
						>
							<p class="text-sm/6 font-medium text-fg-muted">
								{hasActiveFilters ? 'No work matches these filters.' : 'Nothing here.'}
							</p>
							{#if hasActiveFilters}
								<Button
									type="button"
									variant="outline"
									size="sm"
									class="min-h-9 [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
									onclick={clearAllFilters}
								>
									Reset filters
								</Button>
							{/if}
						</div>
					{:else if layout === 'board'}
						<div class="work-board" data-testid="work-board">
							{#each boardColumns as col (col.status)}
								<section
									class="work-bcol"
									data-status={col.status}
									aria-label="{col.label} ({col.items.length})"
								>
									<h2 class="work-bcol-head">
										<span class="work-bcol-title">
											<span class="work-status" data-tone={col.tone} aria-hidden="true">
												{#if col.tone === 'done'}
													<CheckCircle2 class="size-3.5" />
												{:else if col.tone === 'blocked'}
													<PauseCircle class="size-3.5" />
												{:else if col.tone === 'review'}
													<FileCheck2 class="size-3.5" />
												{:else}
													<span class="work-status-ring"></span>
												{/if}
											</span>
											<span>{col.label}</span>
										</span>
										<span class="work-group-cnt tabular-nums">{col.items.length}</span>
									</h2>
									{#if col.items.length === 0}
										<p class="work-bcol-empty">No items</p>
									{:else}
										<ul class="work-bcol-list" role="list">
											{#each col.items as item (item.id)}
												{@const refText = itemRef(item)}
												{@const attention = needsOwnerAttention(item, me)}
												{@const prioWord = priorityWord(item.priority)}
												{@const sevWord =
													item.kind === 'bug' ? severityWord(item.severity) : undefined}
												{@const sevTone =
													item.kind === 'bug' ? severityTone(item.severity) : 'neutral'}
												{@const dotTone = projectDotTone(item.project_id)}
												{@const assigneeName = nameOf(snapshot, item.assignee_id)}
												{@const checkComplete =
													item.checklist.total > 0 && item.checklist.done >= item.checklist.total}
												<li>
													<div
														class="work-bcard"
														data-testid="work-card"
														data-item={item.id}
														data-dot={dotTone}
														aria-current={activeItemId === item.id ? 'true' : undefined}
													>
														<button
															type="button"
															class="work-bcard-title"
															onclick={() => openItem(item)}
														>
															{item.title}
														</button>
														<div class="work-bcard-meta">
															<span class="work-ref">{refText}</span>
															<span class="work-team" data-testid="work-project"
																><span class="min-w-0 [overflow-wrap:anywhere]"
																	>{projectName(item.project_id)}</span
																></span
															>
															{#if item.kind === 'bug' && sevWord}
																<span
																	class={[
																		'inline-flex items-center rounded-full px-2 py-0.5 text-[11px]/4 font-medium',
																		pillToneClass(sevTone)
																	]}>{sevWord}</span
																>
															{:else if prioWord}
																<span
																	class={[
																		'work-prio-pill inline-flex items-center rounded-full px-2 py-0.5 text-[11px]/4',
																		item.priority === 'urgent'
																			? 'is-urgent font-semibold'
																			: 'font-medium'
																	]}>{prioWord}</span
																>
															{/if}
														</div>
														<div class="work-bcard-foot">
															<span class="work-owner">
																<span class="work-owner-av" aria-hidden="true"
																	>{assigneeInitials(assigneeName)}</span
																>
																<span>{assigneeName}</span>
															</span>
															{#if item.checklist.total > 0}
																<span
																	class={['work-criteria', checkComplete && 'is-complete']}
																	title="{item.checklist.done} of {item.checklist.total} steps done"
																>
																	{item.checklist.done}/{item.checklist.total}
																</span>
															{/if}
															{#if attention}
																<span
																	class="t-amber inline-flex items-center rounded-full px-2 py-0.5 text-[11px]/4 font-semibold"
																>
																	Needs you
																</span>
															{/if}
														</div>
													</div>
												</li>
											{/each}
										</ul>
									{/if}
								</section>
							{/each}
						</div>
					{:else}
						<div
							class="work-items"
							data-testid="work-list"
							data-work-dividers={showDividers ? '' : undefined}
						>
							{#each groups as group (group.key)}
								<section class="work-group" aria-label="{group.label} ({group.items.length})">
									<h2 class="work-group-title" data-dot={group.dotTone}>
										{#if group.tone}
											<span class="work-status" data-tone={group.tone} aria-hidden="true">
												{#if group.tone === 'done'}
													<CheckCircle2 class="size-3.5" />
												{:else if group.tone === 'blocked'}
													<PauseCircle class="size-3.5" />
												{:else if group.tone === 'review'}
													<FileCheck2 class="size-3.5" />
												{:else}
													<span class="work-status-ring"></span>
												{/if}
											</span>
										{:else if group.dotTone}
											<span class="work-group-dot" aria-hidden="true"></span>
										{/if}
										<span>{group.label}</span>
										<span class="work-group-cnt tabular-nums">{group.items.length}</span>
									</h2>
									<ul class="work-list" role="list">
										{#each group.items as item (item.id)}
											{@const refText = itemRef(item)}
											{@const ringTone = workStatusTone(item.kind, item.status)}
											{@const statLabel = workStatusLabel(item.kind, item.status)}
											{@const attention = needsOwnerAttention(item, me)}
											{@const meter = priorityMeter(item.priority)}
											{@const prioWord = priorityWord(item.priority)}
											{@const sevWord =
												item.kind === 'bug' ? severityWord(item.severity) : undefined}
											{@const sevTone =
												item.kind === 'bug' ? severityTone(item.severity) : 'neutral'}
											{@const dotTone = projectDotTone(item.project_id)}
											{@const assigneeName = nameOf(snapshot, item.assignee_id)}
											{@const checkComplete =
												item.checklist.total > 0 && item.checklist.done >= item.checklist.total}
											<li class="work-item">
												<div
													class="work-row"
													data-testid="work-row"
													data-item={item.id}
													data-dot={dotTone}
													aria-current={activeItemId === item.id ? 'true' : undefined}
												>
													<div class="work-summary">
														<span class="work-ident">
															{#if item.kind === 'task'}
																{#if meter}
																	<span
																		class="work-priority-mark"
																		data-level={meter.level}
																		data-tone={meter.tone}
																		role="img"
																		aria-label={meter.label}
																		title={meter.label}
																	>
																		<span></span><span></span><span></span>
																	</span>
																{:else}
																	<span class="work-priority-spacer" aria-hidden="true"></span>
																{/if}
															{/if}
															<span class="work-ref">{refText}</span>
														</span>
														<span class="work-heading">
															<span class="work-markers">
																<span
																	class="work-status"
																	data-tone={ringTone}
																	data-attention={attention ? '' : undefined}
																	role="img"
																	aria-label={attention ? `${statLabel}, needs you` : statLabel}
																	title={attention ? `${statLabel} · Needs you` : statLabel}
																>
																	{#if ringTone === 'done'}
																		<CheckCircle2 class="size-3.5" aria-hidden="true" />
																	{:else if ringTone === 'blocked'}
																		<PauseCircle class="size-3.5" aria-hidden="true" />
																	{:else if ringTone === 'review'}
																		<FileCheck2 class="size-3.5" aria-hidden="true" />
																	{:else}
																		<span class="work-status-ring" aria-hidden="true"></span>
																	{/if}
																</span>
																{#if item.checklist.total > 0}
																	<span
																		class={['work-criteria', checkComplete && 'is-complete']}
																		title="{item.checklist.done} of {item.checklist
																			.total} steps done"
																	>
																		{item.checklist.done}/{item.checklist.total}
																	</span>
																{/if}
															</span>
															<button
																type="button"
																class="work-title rounded-sm text-left text-fg"
																onclick={() => openItem(item)}
															>
																{item.title}
															</button>
														</span>
													</div>
													<div class="work-facts">
														{#if item.kind === 'bug' && sevWord}
															<span
																class={[
																	'inline-flex items-center rounded-full px-2 py-0.5 text-[11px]/4 font-medium',
																	pillToneClass(sevTone)
																]}>{sevWord}</span
															>
														{:else if prioWord}
															<span
																class={[
																	'work-prio-pill inline-flex items-center rounded-full px-2 py-0.5 text-[11px]/4',
																	item.priority === 'urgent'
																		? 'is-urgent font-semibold'
																		: 'font-medium'
																]}>{prioWord}</span
															>
														{/if}
														<span class="work-team" data-testid="work-project"
															><span class="min-w-0 [overflow-wrap:anywhere]"
																>{projectName(item.project_id)}</span
															></span
														>
														<span class="work-owner">
															<span class="work-owner-av" aria-hidden="true"
																>{assigneeInitials(assigneeName)}</span
															>
															<span>{assigneeName}</span>
														</span>
													</div>
												</div>
											</li>
										{/each}
									</ul>
								</section>
							{/each}
						</div>
					{/if}
				</div>
			{/if}
		</div>
	</section>
</div>

{#if defaultTaskProject}
	<NewTaskDialog
		bind:open={newTaskOpen}
		projectId={defaultTaskProject.id}
		projectName={defaultTaskProject.name}
		{snapshot}
		{send}
		ondone={(created) => {
			toast.success(
				`Task ${created.ref} created${created.assigneeId ? ` (and given to ${nameOf(snapshot, created.assigneeId)})` : ''}.`
			);
			void load();
		}}
	/>
{/if}
<NewRequestDialog
	bind:open={newRequestOpen}
	{snapshot}
	{find}
	{send}
	ondone={({ ref, sentTo }) => (
		toast.success(sentTo ? `${ref} sent to ${sentTo}.` : `${ref} saved.`),
		void load()
	)}
	onadded={(ref) => (toast.success(`Added to ${ref}.`), void load())}
/>
<RequestSheet
	bind:open={requestOpen}
	{organizationId}
	{requestId}
	start={requestStart}
	{snapshot}
	{me}
	items={items ?? []}
	{send}
	onchanged={(message) => (message && toast.success(message), void load())}
/>
<TaskSheet
	bind:open={taskOpen}
	{organizationId}
	{taskId}
	{snapshot}
	{send}
	onchanged={() => void load()}
/>
<BugSheet
	bind:open={bugOpen}
	{organizationId}
	{bugId}
	{snapshot}
	areas={areasOf(bugProject)}
	bugs={bugsOf(bugProject)}
	items={items ?? []}
	{send}
	onchanged={() => void load()}
/>

<style>
	.work-workspace {
		--work-gutter: 24px;
		--work-control-height: 36px;
		min-width: 0;
		min-height: 29rem;
	}
	@media (max-width: 720px), (pointer: coarse) {
		.work-workspace {
			--work-gutter: 16px;
			--work-control-height: 44px;
		}
	}

	/* Unified Inbox-parity queue header & toolbar (poc/operations.css) */
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
		color: var(--exec-ink);
		font-size: 1.25rem;
		line-height: 1.3;
		font-weight: 650;
		letter-spacing: -0.02em;
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
		flex-wrap: wrap;
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
	.work-search {
		display: flex;
		align-items: center;
		gap: 8px;
		width: min(35rem, 60vw);
		max-width: 100%;
		min-width: 0;
		min-height: max(2.5rem, var(--work-control-height));
		padding: 2px 8px 2px 10px;
		border: 1px solid var(--exec-border-hi);
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-well);
	}
	.work-search:focus-within {
		border-color: var(--focus);
		outline: 2px solid var(--focus);
		outline-offset: -1px;
	}
	.work-search-ico {
		color: var(--exec-muted);
	}
	.work-search-input {
		min-width: 0;
		flex: 1 1 auto;
		width: 100%;
		min-height: var(--work-control-height);
		border: 0;
		background: transparent;
		color: var(--exec-ink);
		font-size: 0.875rem;
		outline: none;
		-webkit-appearance: none;
		appearance: none;
	}
	.work-search-input::-webkit-search-cancel-button {
		-webkit-appearance: none;
		appearance: none;
	}
	.work-search-input::placeholder {
		color: var(--exec-muted);
		opacity: 1;
	}
	.work-search-kbd {
		flex: none;
		padding: 1px 6px;
		border: 1px solid var(--exec-border-hi);
		border-radius: 4px;
		color: var(--exec-muted);
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.75rem;
		line-height: 1.3;
	}
	.work-clear-btn {
		border-radius: var(--exec-sm-radius);
		color: var(--exec-muted);
		cursor: pointer;
	}
	.work-clear-btn:hover {
		color: var(--exec-ink);
		background: var(--exec-btn-ghost-bg-hover);
	}

	/* Work toolbar matching poc/operations.css .work-toolbar.queue-toolbar */
	.work-toolbar {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 6px 12px;
		padding: 8px var(--work-gutter) 10px;
	}
	.queue-tabs {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 2px 4px;
	}
	.queue-tab {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 5px 12px;
		min-height: var(--work-control-height);
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
	.queue-tab:hover:not(.is-disabled) {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.queue-tab:is([aria-selected='true'], [aria-pressed='true']) {
		border-bottom-color: var(--tone-accent-ink);
		box-shadow: inset 0 -1px var(--tone-accent-ink);
		color: var(--exec-ink);
	}
	.queue-tab.is-disabled {
		color: var(--fg-subtle);
		cursor: not-allowed;
	}
	.queue-tab-count {
		color: var(--exec-muted);
		font-variant-numeric: tabular-nums;
		font-size: 0.75rem;
	}
	.queue-tab[aria-selected='true'] .queue-tab-count {
		color: var(--tone-accent-ink);
	}
	.work-layout-toggle {
		display: flex;
		align-items: center;
		gap: 2px;
		padding-inline-start: 10px;
		border-inline-start: 1px solid var(--exec-border);
	}
	.work-layout-toggle .queue-tab {
		border-radius: var(--exec-ctrl-radius);
		padding-inline: 10px;
	}
	.work-layout-toggle .queue-tab[aria-pressed='true'] {
		background: var(--exec-btn-ghost-bg-hover);
		border-color: var(--exec-border-hi);
		box-shadow: none;
		color: var(--exec-ink);
	}
	.queue-controls {
		display: flex;
		align-items: center;
		gap: 4px;
	}
	.queue-icon-btn {
		position: relative;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: var(--work-control-height);
		min-height: var(--work-control-height);
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

	/* Primary Work CTA: >= 4.5:1 WCAG AA contrast at rest & hover across all themes */
	.work-cta {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 6px;
		min-height: var(--work-control-height);
		padding: 0 14px;
		border-radius: var(--exec-ctrl-radius);
		background: var(--primary);
		color: var(--on-emphasis);
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
		transition: background-color 120ms ease-out;
	}
	.work-toolbar-cta {
		margin-inline-start: auto;
	}
	.work-cta:hover {
		background: color-mix(in srgb, var(--primary) 84%, var(--surface));
	}
	:global(html[data-ui-theme='ocean']) .work-cta {
		background: var(--primary-hover);
	}
	:global(html[data-ui-theme='ocean']) .work-cta:hover {
		background: color-mix(in srgb, var(--primary-hover) 84%, var(--surface));
	}
	:global(html[data-ui-theme='report']) .work-cta:hover {
		background: var(--primary-hover);
	}

	@container queue-header (min-width: 52rem) {
		.work-header .queue-headline {
			display: flex;
		}
		.work-header .queue-search-wrap {
			position: static;
			width: min(35rem, 48%);
			margin-inline: auto 0;
			transform: none;
		}
		.work-search {
			width: 100%;
		}
	}
	@container queue-header (min-width: 75rem) {
		.work-header .queue-search-wrap {
			width: clamp(24rem, 29cqi, 35rem);
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
		.work-search {
			width: 100%;
		}
	}
	@container queue-header (max-width: 38rem) {
		.queue-tab {
			gap: 4px;
			padding-inline: 8px;
		}
		.work-layout-toggle {
			padding-inline-start: 0;
			border-inline-start: 0;
		}
	}
	@media (max-width: 720px), (pointer: coarse) {
		.work-search-kbd {
			display: none;
		}
		.work-search-input {
			font-size: 1rem;
		}
	}

	/* Body & saved filter presets row */
	.work-body {
		display: flex;
		flex-direction: column;
		gap: 14px;
		min-width: 0;
		padding: 14px var(--work-gutter) 40px;
	}
	.work-presets-bar {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 8px 12px;
	}
	.work-presets {
		gap: 4px 6px;
	}
	.work-preset {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		min-height: var(--work-control-height);
		padding: 4px 11px;
		border: 1px solid transparent;
		border-radius: var(--exec-ctrl-radius);
		background: transparent;
		color: var(--exec-body);
		font-size: 0.8125rem;
		font-weight: 500;
		white-space: nowrap;
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.work-preset:hover {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.work-preset[aria-pressed='true'] {
		background: var(--tone-accent-bg);
		border-color: var(--tone-accent-ring);
		color: var(--exec-ink);
		font-weight: 600;
	}
	.work-preset-count {
		color: var(--exec-muted);
		font-size: 0.75rem;
	}
	.work-preset[aria-pressed='true'] .work-preset-count {
		color: var(--tone-accent-ink);
	}
	.work-active-summary {
		display: inline-flex;
		align-items: center;
		gap: 10px;
		color: var(--exec-muted);
		font-size: 0.75rem;
	}
	.work-reset-link {
		display: inline-flex;
		align-items: center;
		min-height: var(--work-control-height);
		padding: 0 8px;
		border-radius: var(--exec-sm-radius);
		color: var(--tone-accent-ink);
		font-size: 0.8125rem;
		font-weight: 600;
		cursor: pointer;
	}
	.work-reset-link:hover {
		background: var(--exec-btn-ghost-bg-hover);
		text-decoration: underline;
	}
	.work-results {
		min-width: 0;
	}
	.work-pill-accent {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}

	/* Compact work list (poc/v6.html & poc/operations.css) */
	.work-items {
		container-type: inline-size;
		min-width: 0;
	}
	.work-group + .work-group {
		margin-top: 0.875rem;
	}
	.work-group-title {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.375rem 0.75rem;
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-well);
		font-size: 0.8125rem;
		font-weight: 600;
		line-height: 1.5;
		color: var(--exec-ink);
	}
	.work-group-dot {
		width: 0.5rem;
		height: 0.5rem;
		flex: none;
		border-radius: 50%;
		background: var(--work-dot-color, var(--tone-accent));
	}
	.work-group-title[data-dot='accent'] {
		--work-dot-color: var(--tone-accent);
	}
	.work-group-title[data-dot='ok'] {
		--work-dot-color: var(--tone-ok);
	}
	.work-group-title[data-dot='warn'] {
		--work-dot-color: var(--tone-warn);
	}
	.work-group-title[data-dot='queued'] {
		--work-dot-color: var(--tone-queued);
	}
	.work-group-title[data-dot='bad'] {
		--work-dot-color: var(--tone-bad);
	}
	.work-group-cnt {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: 1.25rem;
		padding: 0 0.375rem;
		border-radius: 999px;
		background: var(--surface-raised);
		color: var(--exec-muted);
		font-size: 0.7rem;
		font-weight: 600;
	}
	.work-list {
		margin: 0.25rem 0 0;
		padding: 0;
		list-style: none;
	}
	.work-row {
		position: relative;
		display: flex;
		align-items: center;
		gap: 0.5rem 1rem;
		min-width: 0;
		min-height: 2.625rem;
		padding: 0.375rem 0.75rem;
		border-radius: var(--exec-sm-radius);
		font-size: 0.8125rem;
		line-height: 1.4;
		transition: background-color 0.12s ease;
	}
	.work-row:is(:hover, :focus-within) {
		background: var(--work-row-hover, var(--surface-raised));
	}
	.work-row[aria-current='true'] {
		background: var(--work-row-selected, var(--tone-accent-bg));
		box-shadow: inset 3px 0 0 var(--work-selected-bar, var(--tone-accent-ink));
	}
	.work-items[data-work-dividers] .work-row {
		border-radius: 0;
		box-shadow: inset 0 -1px var(--line-subtle);
	}
	.work-items[data-work-dividers] .work-row:is(:hover, :focus-within) {
		box-shadow: inset 0 -1px var(--line);
	}
	.work-items[data-work-dividers] .work-row[aria-current='true'] {
		box-shadow:
			inset 3px 0 0 var(--work-selected-bar, var(--tone-accent-ink)),
			inset 0 -1px var(--line);
	}
	.work-summary {
		flex: 1 1 auto;
		display: grid;
		grid-template-columns: auto minmax(0, 1fr);
		align-items: baseline;
		gap: 0.5rem;
		min-width: 0;
		color: var(--exec-ink);
	}
	.work-ident,
	.work-markers {
		display: inline-flex;
		align-items: center;
		gap: 0.4375rem;
	}
	.work-ident {
		min-width: 4.85rem;
	}
	.work-heading {
		display: grid;
		grid-template-columns: auto minmax(0, 1fr);
		align-items: baseline;
		gap: 0.5rem;
		min-width: 0;
	}
	.work-ref {
		min-width: 3.65rem;
		color: var(--exec-muted);
		font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, monospace);
		font-size: 0.75rem;
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
	}
	.work-title {
		font-weight: 600;
		overflow-wrap: anywhere;
		min-height: var(--work-control-height);
		display: inline-flex;
		align-items: center;
		cursor: pointer;
	}
	.work-title::after {
		content: '';
		position: absolute;
		inset: 0;
		border-radius: inherit;
	}
	.work-title:hover {
		color: var(--link);
		text-decoration: underline;
	}
	.work-title:focus-visible {
		outline: none;
	}
	.work-title:focus-visible::after {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	.work-criteria {
		color: var(--exec-muted);
		font-size: 0.75rem;
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
	}
	.work-criteria.is-complete {
		color: var(--tone-ok-ink);
	}
	.work-status,
	.work-priority-mark,
	.work-priority-spacer {
		position: relative;
		display: inline-flex;
		flex: none;
		align-items: center;
		justify-content: center;
		width: 1rem;
		height: 1rem;
		color: var(--exec-muted);
	}
	.work-status[data-tone='active'] {
		color: var(--tone-warn-ink);
	}
	.work-status[data-tone='done'] {
		color: var(--tone-ok-ink);
	}
	.work-status[data-tone='blocked'] {
		color: var(--tone-bad-ink);
	}
	.work-status[data-tone='review'] {
		color: var(--tone-accent-ink);
	}
	.work-status[data-attention]::after {
		content: '';
		position: absolute;
		inset: 0 0 auto auto;
		width: 0.35rem;
		height: 0.35rem;
		border-radius: 50%;
		background: var(--tone-warn-ink);
		box-shadow: 0 0 0 1.5px var(--surface);
	}
	.work-status-ring {
		display: inline-grid;
		place-items: center;
		width: 0.875rem;
		height: 0.875rem;
		border: 1.5px solid currentColor;
		border-radius: 50%;
	}
	.work-status[data-tone='active'] .work-status-ring::before {
		content: '';
		height: 75%;
		aspect-ratio: 1 / 2;
		border-radius: 0 100% 100% 0;
		border-inline-end: 0.24rem solid currentColor;
		transform: translateX(50%);
	}
	.work-priority-mark {
		align-items: flex-end;
		gap: 2px;
		padding-bottom: 3px;
		color: var(--exec-muted);
	}
	.work-priority-mark[data-tone='strong'] {
		color: var(--exec-ink);
	}
	.work-prio-pill {
		background: var(--surface-raised);
		color: var(--fg-secondary);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.work-prio-pill.is-urgent {
		background: var(--surface-overlay);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.work-priority-mark span {
		width: 0.15rem;
		height: 0.28rem;
		border-radius: 1px;
		background: currentColor;
	}
	.work-priority-mark span:nth-child(2) {
		height: 0.44rem;
	}
	.work-priority-mark span:nth-child(3) {
		height: 0.6rem;
	}
	.work-priority-mark[data-level='low'] span:nth-child(n + 2),
	.work-priority-mark[data-level='medium'] span:nth-child(3) {
		opacity: 0.32;
	}
	.work-facts {
		display: flex;
		flex: 0 1 auto;
		justify-content: flex-end;
		align-items: center;
		flex-wrap: wrap;
		gap: 0.25rem 0.625rem;
		min-width: 0;
		color: var(--exec-muted);
		font-size: 0.75rem;
	}
	.work-team {
		display: inline-flex;
		align-items: center;
		gap: 0.375rem;
		max-width: 100%;
		min-width: 0;
		padding: 0.125rem 0.5rem;
		border: 1px solid var(--exec-border-hi);
		border-radius: 0.75rem;
		color: var(--exec-body);
		font-weight: 500;
		white-space: normal;
		overflow-wrap: anywhere;
	}
	.work-team::before {
		content: '';
		width: 0.4375rem;
		height: 0.4375rem;
		flex: none;
		border-radius: 50%;
		background: var(--work-dot-color, var(--tone-accent));
	}
	.work-row[data-dot='accent'],
	.work-bcard[data-dot='accent'] {
		--work-dot-color: var(--tone-accent);
	}
	.work-row[data-dot='ok'],
	.work-bcard[data-dot='ok'] {
		--work-dot-color: var(--tone-ok);
	}
	.work-row[data-dot='warn'],
	.work-bcard[data-dot='warn'] {
		--work-dot-color: var(--tone-warn);
	}
	.work-row[data-dot='queued'],
	.work-bcard[data-dot='queued'] {
		--work-dot-color: var(--tone-queued);
	}
	.work-row[data-dot='bad'],
	.work-bcard[data-dot='bad'] {
		--work-dot-color: var(--tone-bad);
	}
	.work-owner {
		display: inline-flex;
		align-items: center;
		gap: 0.375rem;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.work-owner-av {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 1.375rem;
		height: 1.375rem;
		flex: none;
		border-radius: 999px;
		background: var(--exec-avatar-bg);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		font-size: 0.625rem;
		font-weight: 700;
		letter-spacing: 0.02em;
	}

	/* Board layout (poc/v6.html .board / .bcol / .bcard) */
	.work-board {
		display: grid;
		grid-auto-flow: column;
		grid-auto-columns: minmax(240px, 1fr);
		gap: 14px;
		overflow-x: auto;
		padding-bottom: 8px;
	}
	.work-bcol {
		display: flex;
		flex-direction: column;
		gap: 10px;
		min-height: 180px;
		min-width: 0;
		padding: 12px;
		border-radius: var(--exec-inset-radius);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.work-bcol-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		color: var(--exec-ink);
		font-size: 0.8125rem;
		font-weight: 600;
	}
	.work-bcol-title {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		min-width: 0;
	}
	.work-bcol-list {
		display: flex;
		flex-direction: column;
		gap: 8px;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.work-bcol-empty {
		padding: 12px 4px;
		color: var(--exec-muted);
		font-size: 0.75rem;
	}
	.work-bcard {
		position: relative;
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 10px 12px;
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-surface);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		font-size: 0.8125rem;
		line-height: 1.4;
		transition: background-color 120ms ease-out;
	}
	.work-bcard:is(:hover, :focus-within) {
		background: var(--work-row-hover, var(--exec-surface-hi));
	}
	.work-bcard[aria-current='true'] {
		background: var(--work-row-selected, var(--tone-accent-bg));
		box-shadow:
			inset 3px 0 0 var(--work-selected-bar, var(--tone-accent-ink)),
			inset 0 0 0 1px var(--tone-accent-ring);
	}
	.work-bcard-title {
		display: flex;
		align-items: center;
		min-height: var(--work-control-height);
		text-align: left;
		color: var(--exec-ink);
		font-weight: 600;
		overflow-wrap: anywhere;
		cursor: pointer;
	}
	.work-bcard-title::after {
		content: '';
		position: absolute;
		inset: 0;
		border-radius: inherit;
	}
	.work-bcard-title:hover {
		color: var(--link);
		text-decoration: underline;
	}
	.work-bcard-title:focus-visible {
		outline: none;
	}
	.work-bcard-title:focus-visible::after {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	.work-bcard-meta,
	.work-bcard-foot {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
		color: var(--exec-muted);
		font-size: 0.75rem;
	}

	@container (max-width: 54rem) {
		.work-row {
			align-items: stretch;
			flex-direction: column;
			gap: 0.25rem;
		}
		.work-facts {
			justify-content: flex-start;
		}
	}
	@container (max-width: 27rem) {
		.work-row {
			padding-inline: 0.5rem;
		}
	}
	@container (max-width: 20rem) {
		.work-summary {
			grid-template-columns: minmax(0, 1fr);
			gap: 0.25rem;
		}
	}

	.queue-tab:focus-visible,
	.queue-icon-btn:focus-visible,
	.work-clear-btn:focus-visible,
	.work-preset:focus-visible,
	.work-reset-link:focus-visible,
	.work-cta:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}

	@media (forced-colors: active) {
		.queue-tab,
		.queue-icon-btn,
		.work-search,
		.work-preset,
		.work-cta,
		.work-bcard {
			border: 1px solid ButtonText;
		}
		.queue-tab:is([aria-selected='true'], [aria-pressed='true']),
		.queue-icon-btn[data-active],
		.work-preset[aria-pressed='true'] {
			border: 2px solid Highlight;
			color: Highlight;
		}
		.work-items[data-work-dividers] .work-row {
			border-block-end: 1px solid CanvasText;
		}
		.work-row[aria-current='true'],
		.work-bcard[aria-current='true'] {
			border-inline-start: 3px solid Highlight;
		}
		.work-title:focus-visible::after,
		.work-bcard-title:focus-visible::after {
			outline: 2px solid Highlight;
			outline-offset: 1px;
		}
		.work-row:is(:hover, :focus-within) {
			outline: 1px solid Highlight;
			outline-offset: 1px;
		}
		.work-priority-mark span,
		.work-status[data-attention]::after {
			forced-color-adjust: none;
			background: CanvasText;
		}
		.work-status[data-attention]::after {
			box-shadow: 0 0 0 1px Canvas;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.queue-tab,
		.queue-icon-btn,
		.work-preset,
		.work-cta,
		.work-row,
		.work-bcard {
			transition: none;
		}
	}

	:global(html[data-ui-theme^='report']) .work-page {
		max-width: none;
	}
	:global(html[data-ui-theme^='report']) .work-team {
		border-radius: var(--exec-pill-radius);
	}
</style>
