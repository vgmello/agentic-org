<script lang="ts">
	import { onDestroy, tick, untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import { owner } from '$lib/org/model';
	import BugSheet from '$lib/components/BugSheet.svelte';
	import NewRequestDialog from '$lib/components/NewRequestDialog.svelte';
	import RequestSheet from '$lib/components/RequestSheet.svelte';
	import RequestsPanel from '$lib/components/RequestsPanel.svelte';
	import TaskSheet from '$lib/components/TaskSheet.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { toAppError } from '$lib/office/api';
	import type { WorkItemSummary } from '$lib/proto';
	import { sender, workApi } from '$lib/work';
	import { requestsNeedingYou } from '$lib/work/requests-view';
	import { BUG_STATUS_LABEL, bugRef, severityWord } from '$lib/work/bugs';
	import { nameOf } from '$lib/work/people';
	import { countOf, defaultFilter, needsYou, savedFilters, type WorkTab } from '$lib/work/saved';
	import { priorityWord, STATUS_LABEL, taskRef } from '$lib/work/tasks';
	import { priorityTone, severityTone, statusTone, type WorkTone } from '$lib/work/tones';

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

	async function load(
		targetOrgId = organizationId,
		isCancelled: () => boolean = () => false,
		currentTab = tab
	) {
		const seq = ++loadSeq;
		try {
			const got = (await workApi.snapshot(targetOrgId)).items;
			if (destroyed || isCancelled() || seq !== loadSeq) return;
			items = got;
			now = new Date().toISOString();
			loadError = undefined;
			// Open on the default filter of the tab until the owner picks one.
			if (!chosen) filterId = defaultFilter(currentTab, got, me);
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
			void load(id, () => cancelled, nextTab);
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
		// Only the link's own parameter goes; the rest of the address stays as it was.
		const rest = new URLSearchParams(page.url.searchParams);
		rest.delete('request');
		const nextHref = `${page.url.pathname}${rest.toString() ? `?${rest}` : ''}`;
		void goto(resolve(nextHref as '/work'), {
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
		if (tone === 'accent') return '';
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
	const waiting = $derived(needsYou(items ?? [], me));
	const requestsWaiting = $derived(requestsNeedingYou(items ?? []));
	const bugsOf = (projectId: string | undefined) =>
		(items ?? []).filter((i) => i.kind === 'bug' && i.project_id === projectId);
	const areasOf = (projectId: string | undefined) =>
		(snapshot.areas ?? [])
			.filter((a) => a.project_id === projectId && !a.deleted)
			.map((a) => ({ id: a.id, name: a.name, general: a.general }));
	const bugProject = $derived((items ?? []).find((i) => i.id === bugId)?.project_id);
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
					class="text-2xl font-semibold tracking-tight text-fg focus:outline-none"
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
	</header>

	<div role="tablist" aria-label="Work" class="flex flex-wrap gap-x-1 border-b border-line">
		{#each [['tasks', 'Tasks'], ['bugs', 'Bugs']] as const as [id, name] (id)}
			<button
				id="work-tab-{id}"
				type="button"
				role="tab"
				aria-selected={tab === id}
				tabindex={tab === id ? 0 : -1}
				onclick={() => pick(id)}
				onkeydown={(e) => handleTabKeydown(e, id)}
				class={[
					'-mb-px min-h-9 border-b-2 px-3.5 py-2 text-sm/6 transition-colors focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11',
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
				class="-mb-px inline-flex min-h-9 cursor-not-allowed items-center border-b-2 border-transparent px-3.5 py-2 text-sm/6 text-fg-subtle max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
				>{name}</span
			>
		{/each}
		{#if requestsOn}
			<button
				id="work-tab-requests"
				type="button"
				role="tab"
				aria-selected={tab === 'requests'}
				tabindex={tab === 'requests' ? 0 : -1}
				onclick={() => pick('requests')}
				onkeydown={(e) => handleTabKeydown(e, 'requests')}
				class={[
					'-mb-px inline-flex min-h-9 items-center gap-1.5 border-b-2 px-3.5 py-2 text-sm/6 transition-colors focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11',
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
				class="-mb-px inline-flex min-h-9 cursor-not-allowed items-center border-b-2 border-transparent px-3.5 py-2 text-sm/6 text-fg-subtle max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
				>Requests</span
			>
		{/if}
	</div>

	{#if loadError}
		<div class="exec-card flex flex-col items-center gap-3 px-6 py-10 text-center" role="alert">
			<p class="text-sm/6 font-medium text-danger-text">
				{tab === 'requests' ? "Can't load requests." : loadError}
			</p>
			<Button
				type="button"
				variant="outline"
				class="min-h-9 max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
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
							'work-filter flex min-h-9 w-full items-center justify-between gap-2 rounded-lg px-3 py-1.5 text-left text-sm/6 transition-colors focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11',
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

			{#if shown.length === 0}
				<div
					class="exec-card flex flex-col items-center gap-1.5 px-6 py-14 text-center"
					data-testid="no-match"
				>
					<p class="text-sm/6 font-medium text-fg-muted">Nothing here.</p>
				</div>
			{:else}
				<div class="exec-card overflow-x-auto">
					<table class="w-full text-left text-sm/6" data-testid="work-list">
						<caption class="sr-only">{active.label}</caption>
						<thead
							class="border-b border-line text-[11px]/5 font-semibold tracking-wider text-fg-muted uppercase"
						>
							<tr>
								<th scope="col" class="px-4 py-2.5">Item</th>
								<th scope="col" class="px-4 py-2.5">Title</th>
								<th scope="col" class="px-4 py-2.5">Project</th>
								<th scope="col" class="px-4 py-2.5">
									{tab === 'tasks' ? 'Priority' : 'Severity'}
								</th>
								<th scope="col" class="px-4 py-2.5">Status</th>
								<th scope="col" class="px-4 py-2.5">Assignee</th>
							</tr>
						</thead>
						<tbody class="divide-y divide-line-subtle text-fg-body">
							{#each shown as item (item.id)}
								{@const prioOrSev =
									item.kind === 'bug' ? severityWord(item.severity) : priorityWord(item.priority)}
								{@const prioTone =
									item.kind === 'bug' ? severityTone(item.severity) : priorityTone(item.priority)}
								{@const statLabel =
									item.kind === 'bug'
										? (BUG_STATUS_LABEL[item.status] ?? item.status)
										: (STATUS_LABEL[item.status as 'todo'] ?? item.status)}
								{@const statTone = statusTone(item.kind, item.status)}
								<tr
									data-testid="work-row"
									data-item={item.id}
									class="transition-colors hover:bg-surface-raised"
								>
									<td
										class="px-4 py-3 font-mono text-xs/5 whitespace-nowrap text-fg-muted tabular-nums"
									>
										{item.kind === 'bug'
											? bugRef(item, projectKey(item.project_id))
											: taskRef(item, projectKey(item.project_id))}
									</td>
									<th scope="row" class="px-4 py-3 font-normal">
										<button
											type="button"
											class="rounded-sm text-left font-semibold text-fg hover:text-link hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
											onclick={() =>
												item.kind === 'bug'
													? ((bugId = item.id), (bugOpen = true))
													: ((taskId = item.id), (taskOpen = true))}
										>
											{item.title}
										</button>
									</th>
									<td
										class="px-4 py-3 whitespace-nowrap text-fg-secondary"
										data-testid="work-project"
									>
										{projectName(item.project_id)}
									</td>
									<td class="px-4 py-3 whitespace-nowrap">
										{#if prioOrSev}
											<span
												class={[
													'inline-flex items-center rounded-full px-2 py-0.5 text-xs/4 font-medium',
													prioTone === 'accent' ? 'work-pill-accent' : pillToneClass(prioTone)
												]}>{prioOrSev}</span
											>
										{/if}
									</td>
									<td class="px-4 py-3 whitespace-nowrap">
										<span
											class={[
												'inline-flex items-center rounded-full px-2 py-0.5 text-xs/4 font-medium',
												statTone === 'accent' ? 'work-pill-accent' : pillToneClass(statTone)
											]}>{statLabel}</span
										>
									</td>
									<td class="px-4 py-3 whitespace-nowrap text-fg-muted">
										{nameOf(snapshot, item.assignee_id)}
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</div>
	{/if}
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
	:global(html[data-ui-theme^='report']) .work-page {
		max-width: none;
	}
	:global(html[data-ui-theme^='report']) .work-head {
		border-bottom: 1.5px solid var(--report-masthead-rule);
	}
</style>
