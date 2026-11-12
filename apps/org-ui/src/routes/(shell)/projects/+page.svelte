<script lang="ts">
	import { invalidate } from '$app/navigation';
	import FolderOpen from '@lucide/svelte/icons/folder-open';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import Callout from '$lib/components/Callout.svelte';
	import DividedRow from '$lib/components/DividedRow.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import ProjectTeams from '$lib/components/ProjectTeams.svelte';
	import { orgApi } from '$lib/org/api';
	import { leadsLine, projectRows } from '$lib/org/projects';
	import { sessionFor } from '$lib/org/session';

	let { data } = $props();
	const snapshot = $derived(data.snapshot);
	const rows = $derived(projectRows(snapshot));
	const unassigned = $derived(rows.filter((r) => r.unassigned).length);
	const session = $derived(sessionFor(orgApi, data.connection.organization.id));
	const reload = () => invalidate('app:org');
</script>

<svelte:head><title>Projects · Agentic Org</title></svelte:head>

<div class="space-y-5 px-8 py-6">
	<div class="flex flex-wrap items-start justify-between gap-3">
		<div class="space-y-1">
			<p class="text-sm/6 text-gray-400">What {data.connection.organization.name}'s teams work on. A team's lead also leads its projects.</p>
			{#if data.sample}
				<p class="flex items-center gap-x-2 text-xs/5 text-gray-400" data-testid="sample-data-note"><Badge variant="secondary">Sample data</Badge> An invented organization, not yours.</p>
			{/if}
		</div>
		<div class="text-right">
			<Button disabled aria-describedby="new-project-reason">New project</Button>
			<p id="new-project-reason" class="mt-1 text-xs/5 text-gray-400">Available in a later release.</p>
		</div>
	</div>

	{#if unassigned > 0}
		<Callout data-testid="unassigned-note">
			{#snippet icon()}<TriangleAlert class="size-4 text-amber-300" aria-hidden="true" />{/snippet}
			{unassigned} {unassigned === 1 ? 'project has' : 'projects have'} no team yet.
		</Callout>
	{/if}

	{#if rows.length === 0}
		<div class="flex flex-col items-center px-8 py-16 text-center" data-testid="no-projects">
			<span class="flex size-12 items-center justify-center rounded-full bg-white/5 text-gray-300"><FolderOpen class="size-6" aria-hidden="true" /></span>
			<h2 class="mt-4 text-base/7 font-semibold text-white">No projects yet</h2>
			<p class="mt-1 max-w-sm text-sm/6 text-gray-400">Projects are what teams work on. Creating one comes in a later release.</p>
		</div>
	{:else}
		<ul class="border-y border-white/5" aria-label="Projects">
			{#each rows as row (row.project.id)}
				<DividedRow class="space-y-2" data-testid="project-row" data-project={row.project.id}>
					<div class="flex flex-wrap items-baseline justify-between gap-x-4">
						<a href="/projects/{row.project.id}" class="text-sm/6 font-semibold text-white hover:underline focus-visible:outline-2 focus-visible:outline-indigo-400 [overflow-wrap:anywhere]">{row.project.name}</a>
						<span class="text-xs/5 text-gray-400" data-testid="leads-line">{leadsLine(row)}</span>
					</div>
					<ProjectTeams {row} {snapshot} {session} onchanged={reload} />
				</DividedRow>
			{/each}
		</ul>
	{/if}
	<p class="text-xs/5 text-gray-500">Tasks, bugs and documents are added to projects in a later release.</p>
</div>
