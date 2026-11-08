<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import BugSheet from '$lib/components/BugSheet.svelte';
	import TaskSheet from '$lib/components/TaskSheet.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { toAppError } from '$lib/office/api';
	import type { WorkItemSummary } from '$lib/proto';
	import { sender, workApi } from '$lib/work';
	import { generalAreaId } from '$lib/work/api';
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

	async function load() {
		try {
			items = (await workApi.snapshot(organizationId)).items;
			loadError = undefined;
			// Open on the default filter of the tab until the owner picks one.
			if (!chosen) filterId = defaultFilter(tab, items, me);
		} catch (e) {
			if (items === undefined) loadError = toAppError(e).message;
			else toast.error(toAppError(e).message);
		}
	}
	onMount(load);

	function pick(next: WorkTab) {
		tab = next;
		chosen = false;
		filterId = defaultFilter(next, items ?? [], me);
	}

	const filters = $derived(savedFilters(tab, me));
	const active = $derived(filters.find((f) => f.id === filterId) ?? filters[filters.length - 1]);
	const shown = $derived((items ?? []).filter(active.test).sort((a, b) => projectName(a.project_id).localeCompare(projectName(b.project_id)) || (a.number ?? 0) - (b.number ?? 0)));
	const waiting = $derived(needsYou(items ?? [], me));
	const bugsOf = (projectId: string | undefined) => (items ?? []).filter((i) => i.kind === 'bug' && i.project_id === projectId);
	const areasOf = (projectId: string | undefined) => [{ id: generalAreaId(projectId ?? ''), name: 'General', general: true }];
	const bugProject = $derived((items ?? []).find((i) => i.id === bugId)?.project_id);
</script>

<svelte:head><title>Work · Agentic Org</title></svelte:head>

<div class="space-y-5 px-8 py-6">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<p class="text-sm/6 text-gray-400">Tasks and bugs across every project. <Badge variant="secondary">Sample data</Badge></p>
		<p class="text-sm/6 {waiting > 0 ? 'font-semibold text-amber-200' : 'text-gray-400'}" data-testid="needs-you">{waiting === 0 ? 'Nothing needs you.' : `${waiting} ${waiting === 1 ? 'item needs' : 'items need'} you`}</p>
	</div>

	<div role="tablist" aria-label="Work" class="flex gap-x-1 border-b border-white/10">
		{#each [['tasks', 'Tasks'], ['bugs', 'Bugs']] as const as [id, name] (id)}
			<button type="button" role="tab" aria-selected={tab === id} onclick={() => pick(id)} class="px-3 py-2 text-sm/6 focus-visible:outline-2 focus-visible:outline-indigo-400 {tab === id ? 'border-b-2 border-indigo-400 font-semibold text-white' : 'text-gray-300 hover:text-white'}">{name}</button>
		{/each}
		{#each ['Debt', 'Requests'] as name (name)}
			<span role="tab" aria-selected="false" aria-disabled="true" title="Available in a later release" class="cursor-not-allowed px-3 py-2 text-sm/6 text-gray-500">{name}</span>
		{/each}
	</div>

	{#if loadError}
		<p class="rounded-xl bg-white/5 px-6 py-8 text-center text-sm/6 text-red-300" role="alert">{loadError} <button class="underline" onclick={load}>Try again</button></p>
	{:else if items === undefined}
		<p class="text-sm/6 text-gray-400">Loading work…</p>
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
									<td class="px-4 py-2 text-gray-400">{item.kind === 'bug' ? bugRef(item) : taskRef(item)}</td>
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

<TaskSheet bind:open={taskOpen} {organizationId} {taskId} {snapshot} {send} onchanged={() => void load()} />
<BugSheet bind:open={bugOpen} {organizationId} {bugId} {snapshot} areas={areasOf(bugProject)} bugs={bugsOf(bugProject)} items={items ?? []} {send} onchanged={() => void load()} />
