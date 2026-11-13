<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import BugSheet from '$lib/components/BugSheet.svelte';
	import NewRequestDialog from '$lib/components/NewRequestDialog.svelte';
	import RequestSheet from '$lib/components/RequestSheet.svelte';
	import RequestsPanel from '$lib/components/RequestsPanel.svelte';
	import TaskSheet from '$lib/components/TaskSheet.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { toAppError } from '$lib/office/api';
	import type { WorkItemSummary } from '$lib/proto';
	import { sender, workApi } from '$lib/work';
	import { requestsNeedingYou } from '$lib/work/requests-view';
	import { BUG_STATUS_LABEL, bugRef, severityWord } from '$lib/work/bugs';
	import { nameOf } from '$lib/work/people';
	import { countOf, defaultFilter, needsYou, savedFilters, type WorkTab } from '$lib/work/saved';
	import { priorityWord, STATUS_LABEL, taskRef } from '$lib/work/tasks';

	/** Work across every project (work-items-ux §11): Tasks and Bugs with saved filters; "me" is the owner. */
	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const snapshot = $derived(data.snapshot);
	const me = $derived(snapshot.members.find((m) => m.owner)?.id ?? 'owner');
	const send = $derived(sender(organizationId));
	/** The open requests that read like these words, from the office (offered while typing). */
	const find = (text: string) => workApi.similar(organizationId, { text, limit: 2 }).then((r) => r.items);
	const projectKey = (id: string | undefined) => (snapshot.projects ?? []).find((p) => p.id === id)?.key || undefined;
	const projectName = (id: string | undefined) => (snapshot.projects ?? []).find((p) => p.id === id)?.name ?? 'No project';

	let items = $state<WorkItemSummary[] | undefined>();
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

	async function load() {
		try {
			items = (await workApi.snapshot(organizationId)).items;
			now = new Date().toISOString();
			loadError = undefined;
			// Open on the default filter of the tab until the owner picks one.
			if (!chosen) filterId = defaultFilter(tab, items, me);
		} catch (e) {
			if (items === undefined) loadError = toAppError(e).message;
			else toast.error(toAppError(e).message);
		}
	}
	onMount(() => {
		if (!page.url.searchParams.get('request')) tab = rememberedTab();
		void load();
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
		}
		void goto(page.url.pathname, { replaceState: true, noScroll: true, keepFocus: true });
	});

	// Work opens on the tab it was left on (per organization), as the queue's filters are remembered.
	const TAB_KEY = (id: string) => `agentic-org.work.tab.${id}`;
	function rememberedTab(): WorkTab {
		try {
			const saved = localStorage.getItem(TAB_KEY(organizationId));
			if (saved === 'tasks' || saved === 'bugs' || (saved === 'requests' && workApi.requests)) return saved;
		} catch {
			// unreadable storage: open on Tasks
		}
		return 'tasks';
	}
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

	const filters = $derived(savedFilters(tab, me));
	const active = $derived(filters.find((f) => f.id === filterId) ?? filters[filters.length - 1] ?? { id: '', label: '', test: () => false });
	const shown = $derived((items ?? []).filter(active.test).sort((a, b) => projectName(a.project_id).localeCompare(projectName(b.project_id)) || (a.number ?? 0) - (b.number ?? 0)));
	const waiting = $derived(needsYou(items ?? [], me));
	const requestsWaiting = $derived(requestsNeedingYou(items ?? []));
	const bugsOf = (projectId: string | undefined) => (items ?? []).filter((i) => i.kind === 'bug' && i.project_id === projectId);
	const areasOf = (projectId: string | undefined) => (snapshot.areas ?? []).filter((a) => a.project_id === projectId && !a.deleted).map((a) => ({ id: a.id, name: a.name, general: a.general }));
	const bugProject = $derived((items ?? []).find((i) => i.id === bugId)?.project_id);
</script>

<svelte:head><title>Work · Agentic Org</title></svelte:head>

<div class="space-y-5 px-4 py-6 sm:px-8">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<p class="text-sm/6 text-gray-400">{tab === 'requests' ? 'Requests across every project.' : 'Tasks and bugs across every project.'} <Badge variant="secondary">Sample data</Badge></p>
		<p class="text-sm/6 {waiting > 0 ? 'font-semibold text-amber-200' : 'text-gray-400'}" data-testid="needs-you">{waiting === 0 ? 'Nothing needs you.' : `${waiting} ${waiting === 1 ? 'item needs' : 'items need'} you`}</p>
	</div>

	<div role="tablist" aria-label="Work" class="flex flex-wrap gap-x-1 border-b border-white/10">
		{#each [['tasks', 'Tasks'], ['bugs', 'Bugs']] as const as [id, name] (id)}
			<button type="button" role="tab" aria-selected={tab === id} onclick={() => pick(id)} class="min-h-11 px-3 py-2 text-sm/6 sm:min-h-0 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-400 {tab === id ? 'border-b-2 border-indigo-400 font-semibold text-white' : 'text-gray-300 hover:text-white'}">{name}</button>
		{/each}
		{#each ['Debt'] as name (name)}
			<span role="tab" aria-selected="false" aria-disabled="true" title="Available in a later release" class="inline-flex min-h-11 cursor-not-allowed items-center px-3 py-2 text-sm/6 text-gray-500 sm:min-h-0">{name}</span>
		{/each}
		{#if requestsOn}
			<button type="button" role="tab" aria-selected={tab === 'requests'} onclick={() => pick('requests')} class="min-h-11 px-3 py-2 text-sm/6 sm:min-h-0 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-400 {tab === 'requests' ? 'border-b-2 border-indigo-400 font-semibold text-white' : 'text-gray-300 hover:text-white'}">Requests{#if requestsWaiting > 0}<span class="ml-1.5 rounded-full bg-amber-500/20 px-1.5 text-xs/5 text-amber-200" data-testid="requests-badge">{requestsWaiting}</span>{/if}</button>
		{/if}
		{#if !requestsOn}
			<span role="tab" aria-selected="false" aria-disabled="true" title="Available in a later release" class="inline-flex min-h-11 cursor-not-allowed items-center px-3 py-2 text-sm/6 text-gray-500 sm:min-h-0">Requests</span>
		{/if}
	</div>

	{#if loadError}
		<p class="rounded-xl bg-white/5 px-6 py-8 text-center text-sm/6 text-red-300" role="alert">{tab === 'requests' ? "Can't load requests." : loadError} <button class="underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400" onclick={load}>{tab === 'requests' ? 'Retry' : 'Try again'}</button></p>
	{:else if items === undefined}
		{#if tab === 'requests'}
			<ul class="divide-y divide-white/5 rounded-xl bg-white/5" aria-busy="true" aria-label="Loading requests" data-testid="requests-loading">
				{#each [0, 1, 2] as n (n)}<li class="flex items-center gap-x-3 px-4 py-3"><span class="h-4 w-14 animate-pulse rounded bg-white/10"></span><span class="h-4 flex-1 animate-pulse rounded bg-white/10"></span><span class="h-4 w-24 animate-pulse rounded bg-white/10"></span></li>{/each}
			</ul>
		{:else}
			<p class="text-sm/6 text-gray-400">Loading work…</p>
		{/if}
	{:else if tab === 'requests'}
		{#key organizationId}<RequestsPanel items={items} {snapshot} {now} {organizationId} onopen={(id, start) => ((requestId = id), (requestStart = start), (requestOpen = true))} onnew={() => (newRequestOpen = true)} />{/key}
	{:else if (snapshot.projects ?? []).length === 0}
		<p class="rounded-xl bg-white/5 px-6 py-8 text-center text-sm/6 text-gray-300" data-testid="no-projects">No projects yet. Work items live in projects.</p>
	{:else}
		<div class="grid gap-5 md:grid-cols-[14rem_1fr]">
			<nav aria-label="Saved filters" class="space-y-1">
				{#each filters as f (f.id)}
					<button type="button" aria-pressed={active.id === f.id} data-filter={f.id} onclick={() => ((chosen = true), (filterId = f.id))} class="flex w-full items-center justify-between rounded-md px-3 py-1.5 text-left text-sm/6 focus-visible:outline-2 focus-visible:outline-indigo-400 {active.id === f.id ? 'bg-white/10 font-semibold text-white' : 'text-gray-300 hover:bg-white/5'}">
						<span>{f.label}</span><span class="text-gray-400">{countOf(items, f)}</span>
					</button>
				{/each}
			</nav>

			{#if shown.length === 0}
				<p class="rounded-xl bg-white/5 px-6 py-8 text-center text-sm/6 text-gray-400" data-testid="no-match">Nothing here.</p>
			{:else}
				<div class="overflow-x-auto rounded-xl bg-white/5 outline-1 -outline-offset-1 outline-white/10">
					<table class="w-full text-left text-sm/6" data-testid="work-list">
						<caption class="sr-only">{active.label}</caption>
						<thead class="text-xs/5 text-gray-400">
							<tr>
								<th scope="col" class="px-4 py-2 font-medium">Item</th>
								<th scope="col" class="px-4 py-2 font-medium">Title</th>
								<th scope="col" class="px-4 py-2 font-medium">Project</th>
								<th scope="col" class="px-4 py-2 font-medium">{tab === 'tasks' ? 'Priority' : 'Severity'}</th>
								<th scope="col" class="px-4 py-2 font-medium">Status</th>
								<th scope="col" class="px-4 py-2 font-medium">Assignee</th>
							</tr>
						</thead>
						<tbody class="divide-y divide-white/5 text-gray-200">
							{#each shown as item (item.id)}
								<tr data-testid="work-row" data-item={item.id}>
									<td class="px-4 py-2 text-gray-400">{item.kind === 'bug' ? bugRef(item, projectKey(item.project_id)) : taskRef(item, projectKey(item.project_id))}</td>
									<th scope="row" class="px-4 py-2 font-normal"><button type="button" class="text-left font-semibold text-white hover:underline focus-visible:outline-2 focus-visible:outline-indigo-400 [overflow-wrap:anywhere]" onclick={() => (item.kind === 'bug' ? ((bugId = item.id), (bugOpen = true)) : ((taskId = item.id), (taskOpen = true)))}>{item.title}</button></th>
									<td class="px-4 py-2" data-testid="work-project">{projectName(item.project_id)}</td>
									<td class="px-4 py-2">{item.kind === 'bug' ? severityWord(item.severity) : (priorityWord(item.priority) ?? '')}</td>
									<td class="px-4 py-2">{item.kind === 'bug' ? (BUG_STATUS_LABEL[item.status] ?? item.status) : (STATUS_LABEL[item.status as 'todo'] ?? item.status)}</td>
									<td class="px-4 py-2 text-gray-400">{nameOf(snapshot, item.assignee_id)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</div>
	{/if}
</div>

<NewRequestDialog bind:open={newRequestOpen} {snapshot} {find} {send} ondone={({ ref, sentTo }) => (toast.success(sentTo ? `${ref} sent to ${sentTo}.` : `${ref} saved.`), void load())} onadded={(ref) => (toast.success(`Added to ${ref}.`), void load())} />
<RequestSheet bind:open={requestOpen} {organizationId} {requestId} start={requestStart} {snapshot} items={items ?? []} {send} onchanged={(message) => (message && toast.success(message), void load())} />
<TaskSheet bind:open={taskOpen} {organizationId} {taskId} {snapshot} {send} onchanged={() => void load()} />
<BugSheet bind:open={bugOpen} {organizationId} {bugId} {snapshot} areas={areasOf(bugProject)} bugs={bugsOf(bugProject)} items={items ?? []} {send} onchanged={() => void load()} />
