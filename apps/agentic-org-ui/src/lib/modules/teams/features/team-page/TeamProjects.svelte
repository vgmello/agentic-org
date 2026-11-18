<!--
  @component
  A team's Projects tab (§7.2, §7.9, TEAM-13): each active project it owns or contributes to, as divided rows:
  name (opens the project) and key, Owner or Contributor in words, the accountable owner team when this team only
  contributes, and the number of open tasks. Owner and contributor teams are edited in the project's settings.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { teamsApi } from '../../api';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import { teamPageContext } from './context';

	const team = teamPageContext();

	const projects = new Resource(() => teamsApi.projects(team.organizationId, team.teamId));
	const openWork = (n: number) =>
		n === 0 ? 'No open tasks' : `${n} open ${n === 1 ? 'task' : 'tasks'}`;
</script>

<Loaded
	errorLevel={4}
	resource={projects}
	label="the team's projects"
	error="Couldn't load the team's projects"
>
	{#snippet children(rows)}
		{#if rows.length === 0}
			<EmptyState
				level={4}
				title="No projects yet"
				description="Projects {team.teamName} owns or contributes to show here. A project's settings choose its owner team and contributing teams."
			>
				{#snippet action()}
					<Button variant="outline" href={resolve('/projects')} class="max-[639px]:h-11"
						>Open Projects</Button
					>
				{/snippet}
			</EmptyState>
		{:else}
			<ul role="list" class="divide-y divide-white/5">
				{#each rows as p (p.id)}
					<li
						class="relative flex flex-wrap items-center justify-between gap-x-4 gap-y-1 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8"
					>
						<div class="min-w-0 flex-1 basis-60">
							<p class="flex flex-wrap items-baseline gap-x-2 text-sm/6">
								<a
									href={resolve(`/projects/${encodeURIComponent(p.id)}` as '/')}
									class="rounded-sm font-semibold text-white focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-400 [overflow-wrap:anywhere]"
									><span class="absolute inset-0" aria-hidden="true"></span>{p.name}</a
								>
								{#if p.key}<span class="font-mono text-xs/5 text-gray-400">{p.key}</span>{/if}
							</p>
							<p class="mt-1 flex flex-wrap items-center gap-x-2 text-xs/5 text-gray-400">
								<span class="font-medium text-gray-300"
									>{p.role === 'owner' ? 'Owner' : 'Contributor'}</span
								>
								{#if p.ownerTeam}
									<span aria-hidden="true">·</span>
									<span
										>Owned by <a
											href={resolve(`/teams/${encodeURIComponent(p.ownerTeam.id)}` as '/')}
											class="relative inline-flex min-h-6 items-center rounded-sm text-indigo-400 max-[639px]:min-h-11 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
											>{p.ownerTeam.name}</a
										>{#if p.ownerTeam.ref}<span class="ml-1 font-mono">{p.ownerTeam.ref}</span
											>{/if}</span
									>
								{/if}
								<span aria-hidden="true">·</span>
								<span class="tabular-nums">{openWork(p.openWork)}</span>
							</p>
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	{/snippet}
</Loaded>
