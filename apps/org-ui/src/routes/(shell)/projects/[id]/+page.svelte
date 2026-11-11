<script lang="ts">
	import { invalidate } from '$app/navigation';
	import { Badge } from '$lib/components/ui/badge';
	import Callout from '$lib/components/Callout.svelte';
	import DividedRow from '$lib/components/DividedRow.svelte';
	import FolderOpen from '@lucide/svelte/icons/folder-open';
	import Info from '@lucide/svelte/icons/info';
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

<div class="min-w-0 space-y-5 px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8">
	<nav aria-label="Breadcrumb" class="min-w-0 text-sm/6 text-gray-400 [overflow-wrap:anywhere]"><a href="/projects" class="hover:text-white hover:underline focus-visible:outline-2 focus-visible:outline-indigo-400">Projects</a> › <span class="text-gray-200">{row?.project.name ?? ''}</span></nav>

	{#if !row}
		<Callout data-testid="project-gone">
			{#snippet icon()}<FolderOpen class="size-4 text-gray-400" aria-hidden="true" />{/snippet}
			<p class="text-sm/6 font-semibold text-white">This project isn't here any more.</p>
			<a href="/projects" class="mt-2 inline-block max-w-full rounded-sm text-sm/6 text-indigo-300 underline underline-offset-2 focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-indigo-400">Go to Projects</a>
		</Callout>
	{:else}
		<header class="flex flex-wrap items-center gap-x-3 gap-y-2">
			<h2 class="min-w-0 text-base/7 font-semibold text-white [overflow-wrap:anywhere]" data-testid="project-name">{row.project.name}</h2>
			{#if data.sample}<Badge variant="secondary">Sample data</Badge>{/if}
		</header>
		<ul role="list" aria-label="Project details" class="border-y border-white/5">
			<DividedRow class="space-y-2 px-0 sm:px-0 lg:px-0">
				<ProjectTeams {row} snapshot={data.snapshot} {session} onchanged={reload} />
				<p class="text-sm/6 text-gray-300" data-testid="leads-line">{leadsLine(row)}</p>
			</DividedRow>
		</ul>

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
			<Callout>
				{#snippet icon()}<Info class="size-4 text-indigo-300" aria-hidden="true" />{/snippet}
				Features, bugs, debt, documents and insights for this project are available in a later release.
			</Callout>
		{/if}
	{/if}
</div>
