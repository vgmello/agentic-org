<script lang="ts">
	import { invalidate } from '$app/navigation';
	import { Badge } from '$lib/components/ui/badge';
	import ProjectTeams from '$lib/components/ProjectTeams.svelte';
	import BugsPanel from '$lib/components/BugsPanel.svelte';
	import TasksBoard from '$lib/components/TasksBoard.svelte';
	import { orgApi } from '$lib/org/api';
	import { leadsLine, projectById } from '$lib/org/projects';
	import { sessionFor } from '$lib/org/session';
	import { page } from '$app/state';

	let { data } = $props();
	const row = $derived(projectById(data.snapshot, page.params.id ?? ''));
	const session = $derived(sessionFor(orgApi, data.connection.organization.id));
	const reload = () => invalidate('app:org');
	// What a project will hold once the work-items contract exists: shown, not hidden, and disabled with the reason.
	const COMING = ['Features', 'Debt', 'Docs', 'Insights', 'Settings'];
	let tab = $state<'overview' | 'tasks' | 'bugs'>('overview');
</script>

<svelte:head><title>{row?.project.name ?? 'Project'} · Agentic Org</title></svelte:head>

<div class="space-y-5 px-8 py-6">
	<nav aria-label="Breadcrumb" class="text-sm/6 text-gray-400"><a href="/projects" class="hover:text-white hover:underline focus-visible:outline-2 focus-visible:outline-indigo-400">Projects</a> › <span class="text-gray-200">{row?.project.name ?? ''}</span></nav>

	{#if !row}
		<div class="rounded-xl bg-white/5 px-6 py-10 text-center outline-1 -outline-offset-1 outline-white/10" data-testid="project-gone">
			<p class="text-sm/6 font-semibold text-white">This project isn't here any more.</p>
			<a href="/projects" class="mt-2 inline-block text-sm/6 text-indigo-300 underline underline-offset-2">Go to Projects</a>
		</div>
	{:else}
		<header class="space-y-3 rounded-xl bg-white/5 p-4 outline-1 -outline-offset-1 outline-white/10">
			<div class="flex flex-wrap items-center gap-x-3">
				<h2 class="text-lg/7 font-semibold text-white [overflow-wrap:anywhere]" data-testid="project-name">{row.project.name}</h2>
				{#if data.sample}<Badge variant="secondary">Sample data</Badge>{/if}
			</div>
			<ProjectTeams {row} snapshot={data.snapshot} {session} onchanged={reload} />
			<p class="text-sm/6 text-gray-300" data-testid="leads-line">{leadsLine(row)}</p>
		</header>

		<div role="tablist" aria-label="Project" class="flex flex-wrap gap-x-1 border-b border-white/10">
			{#each [['overview', 'Overview'], ['tasks', 'Tasks'], ['bugs', 'Bugs']] as const as [id, name] (id)}
				<button type="button" role="tab" aria-selected={tab === id} onclick={() => (tab = id)} class="px-3 py-2 text-sm/6 focus-visible:outline-2 focus-visible:outline-indigo-400 {tab === id ? 'border-b-2 border-indigo-400 font-semibold text-white' : 'text-gray-300 hover:text-white'}">{name}</button>
			{/each}
			{#each COMING as name (name)}
				<span role="tab" aria-selected="false" aria-disabled="true" title="Available in a later release" class="cursor-not-allowed px-3 py-2 text-sm/6 text-gray-500">{name}</span>
			{/each}
		</div>
		{#if tab === 'tasks'}
			<TasksBoard organizationId={data.connection.organization.id} projectId={row.project.id} projectName={row.project.name} snapshot={data.snapshot} />
		{:else if tab === 'bugs'}
			<BugsPanel organizationId={data.connection.organization.id} projectId={row.project.id} projectName={row.project.name} snapshot={data.snapshot} />
		{:else}
			<p class="text-sm/6 text-gray-400">Features, bugs, debt, documents and insights for this project are available in a later release.</p>
		{/if}
	{/if}
</div>
