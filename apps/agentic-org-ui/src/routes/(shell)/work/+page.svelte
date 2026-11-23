<script lang="ts">
	import { onDestroy, tick, untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import FileCheck2 from '@lucide/svelte/icons/file-check-2';
	import PauseCircle from '@lucide/svelte/icons/pause-circle';
	import Plus from '@lucide/svelte/icons/plus';
	import SlidersHorizontal from '@lucide/svelte/icons/sliders-horizontal';
	import { toast } from 'svelte-sonner';
	import { owner } from '$lib/org/model';
	import BugSheet from '$lib/components/BugSheet.svelte';
	import NewRequestDialog from '$lib/components/NewRequestDialog.svelte';
	import RequestSheet from '$lib/components/RequestSheet.svelte';
	import RequestsPanel from '$lib/components/RequestsPanel.svelte';
	import TaskSheet from '$lib/components/TaskSheet.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { toAppError } from '$lib/office/api';
	import type { WorkItemSummary } from '$lib/proto';
	import { sender, workApi } from '$lib/work';
	import { requestsNeedingYou } from '$lib/work/requests-view';
	import { bugRef, severityWord } from '$lib/work/bugs';
	import {
		assigneeInitials,
		groupWorkByStatus,
		needsOwnerAttention,
		priorityMeter,
		projectDotTone,
		workStatusLabel,
		workStatusTone
	} from '$lib/work/compact';
	import { nameOf } from '$lib/work/people';
	import { countOf, defaultFilter, needsYou, savedFilters, type WorkTab } from '$lib/work/saved';
	import { priorityWord, taskRef } from '$lib/work/tasks';
	import { severityTone, type WorkTone } from '$lib/work/tones';

	/** Work across every project (work-items-ux §11): Tasks and Bugs with saved filters; "me" is the owner. */
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

	let items = $state.raw<WorkItemSummary[] | undefined>();
	let loadError = $state<string | undefined>();
	let tab = $state<WorkTab>('tasks');
	let filterId = $state('all');
	let chosen = false;
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
	const requestsOn = workApi.requests;

	// Display preference: optional row dividers (persisted locally across sessions).
	const DIVIDERS_KEY = 'agentic-org.work.dividers';
	function readDividers(): boolean {
		try {
			return localStorage.getItem(DIVIDERS_KEY) === '1';
		} catch {
			return false;
		}
	}
	let showDividers = $state(readDividers());
	function toggleDividers(next: boolean) {
		showDividers = next;
		try {
			localStorage.setItem(DIVIDERS_KEY, next ? '1' : '0');
		} catch {
			// kept for this session only
		}
	}

	// Work opens on the tab it was left on (per organization), as the queue's filters are remembered.
	const TAB_KEY = (id: string) => `agentic-org.work.tab.${id}`;
	function rememberedTab(orgId: string): WorkTab {
		try {
			const saved = localStorage.getItem(TAB_KEY(orgId));
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
			localStorage.setItem(TAB_KEY(organizationId), next);
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

	function pillToneClass(tone: WorkTone): string {
		if (tone === 'ok') return 't-green';
		if (tone === 'warn') return 't-amber';
		if (tone === 'bad') return 't-red';
		if (tone === 'accent') return 'work-pill-accent';
		return 'bg-surface-raised text-fg-secondary ring-1 ring-line ring-inset';
	}

	const filters = $derived(savedFilters(tab, me));
	const active = $derived(
		filters.find((f) => f.id === filterId) ??
			filters[filters.length - 1] ?? { id: '', label: '', test: () => false }
	);
	const shown = $derived(
		(items ?? [])
			.filter(active.test)
			.sort(
				(a, b) =>
					projectName(a.project_id).localeCompare(projectName(b.project_id)) ||
					(a.number ?? 0) - (b.number ?? 0)
			)
	);
	const groups = $derived(tab === 'requests' ? [] : groupWorkByStatus(shown, tab));
	const waiting = $derived(needsYou(items ?? [], me));
	const requestsWaiting = $derived(requestsNeedingYou(items ?? []));
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

<div class="work-page flex w-full flex-col gap-5 px-4 pt-3 pb-12 sm:px-8">
	<header
		class="work-head flex flex-wrap items-end justify-between gap-4 border-b border-line pb-4"
	>
		<div class="min-w-0">
			<div class="flex flex-wrap items-center gap-2.5">
				<h1
					id="shell-page-title"
					tabindex="-1"
					class="text-xl/7 font-semibold tracking-tight text-fg focus:outline-none sm:text-2xl"
				>
					Work
				</h1>
				<Badge variant="secondary">Sample data</Badge>
			</div>
			<p class="mt-0.5 text-sm/6 text-fg-muted">
				{tab === 'requests'
					? 'Requests across every project.'
					: 'Tasks and bugs across every project.'}
			</p>
		</div>
		<div class="flex flex-wrap items-center gap-2.5">
			<p
				class={[
					'inline-flex min-h-7 items-center rounded-full px-2.5 py-0.5 text-xs/5 font-semibold tabular-nums',
					waiting > 0 ? 't-amber' : 'bg-surface-raised text-fg-muted ring-1 ring-line ring-inset'
				]}
				data-testid="needs-you"
			>
				{waiting === 0
					? 'Nothing needs you.'
					: `${waiting} ${waiting === 1 ? 'item needs' : 'items need'} you`}
			</p>
			{#if requestsOn && tab !== 'requests'}
				<button
					type="button"
					class="work-cta inline-flex min-h-9 items-center justify-center gap-1.5 rounded-lg px-3.5 text-sm/5 font-semibold transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
					onclick={() => (newRequestOpen = true)}
				>
					<Plus class="size-4 shrink-0" aria-hidden="true" />
					<span>New request</span>
				</button>
			{/if}
		</div>
	</header>

	<div role="tablist" aria-label="Work" class="flex flex-wrap gap-x-1 border-b border-line">
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
				class={[
					'-mb-px min-h-9 border-b-2 px-3.5 py-2 text-sm/6 transition-colors focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11',
					tab === id
						? 'border-brand font-semibold text-fg'
						: 'border-transparent text-fg-muted hover:border-line-strong hover:text-fg'
				]}>{name}</button
			>
		{/each}
		{#each ['Debt'] as name (name)}
			<span
				role="tab"
				aria-selected="false"
				aria-disabled="true"
				title="Available in a later release"
				class="-mb-px inline-flex min-h-9 cursor-not-allowed items-center border-b-2 border-transparent px-3.5 py-2 text-sm/6 text-fg-subtle max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
				>{name}</span
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
				class={[
					'-mb-px inline-flex min-h-9 items-center gap-1.5 border-b-2 px-3.5 py-2 text-sm/6 transition-colors focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11',
					tab === 'requests'
						? 'border-brand font-semibold text-fg'
						: 'border-transparent text-fg-muted hover:border-line-strong hover:text-fg'
				]}
				>Requests{#if requestsWaiting > 0}<span
						class="t-amber inline-flex items-center rounded-full px-1.5 py-0.5 text-[11px]/4 font-semibold tabular-nums"
						data-testid="requests-badge">{requestsWaiting}</span
					>{/if}</button
			>
		{/if}
		{#if !requestsOn}
			<span
				role="tab"
				aria-selected="false"
				aria-disabled="true"
				title="Available in a later release"
				class="-mb-px inline-flex min-h-9 cursor-not-allowed items-center border-b-2 border-transparent px-3.5 py-2 text-sm/6 text-fg-subtle max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
				>Requests</span
			>
		{/if}
	</div>

	<div
		id="work-panel"
		role="tabpanel"
		aria-labelledby="work-tab-{tab}"
		tabindex={panelNeedsFocus ? 0 : -1}
		class="focus-visible:rounded-lg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
	>
		{#if loadError}
			<div class="exec-card flex flex-col items-center gap-3 px-6 py-10 text-center" role="alert">
				<p class="text-sm/6 font-medium text-danger-text">
					{tab === 'requests' ? "Can't load requests." : loadError}
				</p>
				<Button
					type="button"
					variant="outline"
					class="min-h-9 max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
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
					onopen={(id, start) => ((requestId = id), (requestStart = start), (requestOpen = true))}
					onnew={() => (newRequestOpen = true)}
				/>{/key}
		{:else if (snapshot.projects ?? []).length === 0}
			<div
				class="exec-card flex flex-col items-center gap-1.5 px-6 py-14 text-center"
				data-testid="no-projects"
			>
				<p class="text-sm/6 font-semibold text-fg">No projects yet. Work items live in projects.</p>
			</div>
		{:else}
			<div class="grid items-start gap-5 md:grid-cols-[14.5rem_minmax(0,1fr)]">
				<nav aria-label="Saved filters" class="flex flex-col gap-1">
					{#each filters as f (f.id)}
						{@const n = countOf(items, f)}
						<button
							type="button"
							aria-pressed={active.id === f.id}
							data-filter={f.id}
							onclick={() => ((chosen = true), (filterId = f.id))}
							class={[
								'work-filter flex min-h-9 w-full items-center justify-between gap-2 rounded-lg px-3 py-1.5 text-left text-sm/6 transition-colors focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11',
								active.id === f.id
									? 'is-on font-semibold text-fg'
									: 'text-fg-secondary hover:bg-surface-raised hover:text-fg'
							]}
						>
							<span class="truncate">{f.label}</span>
							<span
								class={[
									'shrink-0 rounded-full px-2 py-0.5 text-xs/4 tabular-nums',
									active.id === f.id
										? 'work-filter-count font-semibold'
										: 'bg-surface-raised text-fg-muted'
								]}>{n}</span
							>
						</button>
					{/each}
				</nav>

				<div class="flex min-w-0 flex-col gap-3">
					<div class="flex flex-wrap items-center justify-between gap-2">
						<p class="text-xs/5 font-medium text-fg-muted tabular-nums">
							{active.label} · {shown.length}
						</p>
						<DropdownMenu.Root>
							<DropdownMenu.Trigger>
								{#snippet child({ props })}
									<Button
										{...props}
										variant="outline"
										size="sm"
										class="min-h-9 gap-1.5 max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
									>
										<SlidersHorizontal data-icon="inline-start" aria-hidden="true" />
										Display
									</Button>
								{/snippet}
							</DropdownMenu.Trigger>
							<DropdownMenu.Content align="end" class="w-48">
								<DropdownMenu.Group>
									<DropdownMenu.GroupHeading>Work display</DropdownMenu.GroupHeading>
									<DropdownMenu.CheckboxItem
										checked={showDividers}
										onCheckedChange={(checked) => toggleDividers(checked === true)}
										class="min-h-9 max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
									>
										Row dividers
									</DropdownMenu.CheckboxItem>
								</DropdownMenu.Group>
							</DropdownMenu.Content>
						</DropdownMenu.Root>
					</div>

					{#if shown.length === 0}
						<div
							class="exec-card flex flex-col items-center gap-1.5 px-6 py-14 text-center"
							data-testid="no-match"
						>
							<p class="text-sm/6 font-medium text-fg-muted">Nothing here.</p>
						</div>
					{:else}
						<div
							class="work-items exec-card overflow-hidden p-2 sm:p-3"
							data-testid="work-list"
							data-work-dividers={showDividers ? '' : undefined}
						>
							{#each groups as group (group.status)}
								<section class="work-group" aria-label="{group.label} ({group.items.length})">
									<h2 class="work-group-title">
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
										<span>{group.label}</span>
										<span class="work-group-cnt tabular-nums">{group.items.length}</span>
									</h2>
									<ul class="work-list" role="list">
										{#each group.items as item (item.id)}
											{@const refText =
												item.kind === 'bug'
													? bugRef(item, projectKey(item.project_id))
													: taskRef(item, projectKey(item.project_id))}
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
																class="work-title rounded-sm text-left text-fg hover:text-link hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
																onclick={() =>
																	item.kind === 'bug'
																		? ((bugId = item.id), (bugOpen = true))
																		: ((taskId = item.id), (taskOpen = true))}
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
			</div>
		{/if}
	</div>
</div>

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
	/* Primary Work CTA: >= 4.5:1 WCAG AA contrast at rest & hover across all 4 themes */
	.work-cta {
		background: var(--primary);
		color: var(--on-emphasis);
		cursor: pointer;
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
	:global(html[data-ui-theme='report']) .work-cta {
		border-radius: var(--exec-ctrl-radius);
	}
	:global(html[data-ui-theme='report']) .work-cta:hover {
		background: var(--primary-hover);
	}
	.work-filter.is-on {
		background: var(--tone-accent-bg);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
	.work-filter-count {
		color: var(--tone-accent-ink);
	}
	.work-pill-accent {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}

	/* Compact work rows ported from poc/v6.html — token-driven across all 4 themes */
	.work-items {
		container-type: inline-size;
		min-width: 0;
	}
	.work-group + .work-group {
		margin-top: 0.625rem;
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
		margin: 0.125rem 0 0;
		padding: 0;
		list-style: none;
	}
	.work-row {
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
		background: var(--surface-raised);
	}
	.work-items[data-work-dividers] .work-row {
		border-radius: 0;
		box-shadow: inset 0 -1px var(--line-subtle);
	}
	.work-items[data-work-dividers] .work-row:is(:hover, :focus-within) {
		box-shadow: inset 0 -1px var(--line);
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
		min-height: 2.25rem;
		display: inline-flex;
		align-items: center;
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
	.work-row[data-dot='accent'] {
		--work-dot-color: var(--tone-accent);
	}
	.work-row[data-dot='ok'] {
		--work-dot-color: var(--tone-ok);
	}
	.work-row[data-dot='warn'] {
		--work-dot-color: var(--tone-warn);
	}
	.work-row[data-dot='queued'] {
		--work-dot-color: var(--tone-queued);
	}
	.work-row[data-dot='bad'] {
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
	@media (width < 40rem), (pointer: coarse) {
		.work-title {
			min-height: 2.75rem;
		}
	}
	@media (forced-colors: active) {
		[role='tab'][aria-selected='true'] {
			border-bottom-color: Highlight;
			color: Highlight;
		}
		.work-filter[aria-pressed='true'] {
			border: 1px solid Highlight;
			color: Highlight;
		}
		.work-cta {
			border: 1px solid ButtonText;
		}
		.work-items[data-work-dividers] .work-row {
			border-block-end: 1px solid CanvasText;
		}
		.work-row:is(:hover, :focus-within) {
			outline: 1px solid Highlight;
			outline-offset: -1px;
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
	:global(html[data-ui-theme^='report']) .work-page {
		max-width: none;
	}
	:global(html[data-ui-theme^='report']) .work-head {
		border-bottom: 1.5px solid var(--report-masthead-rule);
	}
	:global(html[data-ui-theme^='report']) .work-team {
		border-radius: var(--exec-pill-radius);
	}
</style>
