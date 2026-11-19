<!--
  @component
  A team's Projects tab (§7.2, §7.9, TEAM-13): each active project it owns or contributes to, as divided rows:
  name (opens the project) and key, Owner or Contributor in words, the accountable owner team when this team only
  contributes, the other contributing teams, each a link, and the number of open tasks. Owner and contributor teams are edited in the project's settings.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { teamsApi } from '../../api';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import { teamHref } from '../../team-page';
	import type { TeamRef } from '$lib/modules/members';
	import { teamPageContext } from './context';
	import { reloadOnChange } from './reload.svelte';
	import { fact, factsList, factsWrap, linkInLine } from './facts';

	const team = teamPageContext();

	const projects = new Resource(() => teamsApi.projects(team.organizationId, team.teamId));
	reloadOnChange(projects, () => team.loads);
	const openWork = (n: number) =>
		n === 0 ? 'No open tasks' : `${n} open ${n === 1 ? 'task' : 'tasks'}`;
</script>

<!-- A team named in a row's facts: a link inside the line, above the row's stretched link (TEAM-13). -->
{#snippet teamLink(t: TeamRef)}<a
		href={resolve(teamHref(t.id) as '/')}
		class="relative {linkInLine.xs}">{t.name}</a
	>{#if t.ref}<span class="ml-1 font-mono">{t.ref}</span>{/if}{/snippet}

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
			<ul role="list" class="divide-y divide-line-subtle">
				{#each rows as p (p.id)}
					<li
						class="relative flex flex-wrap items-center justify-between gap-x-4 gap-y-1 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8"
					>
						<div class="min-w-0 flex-1 basis-60">
							<p class="flex flex-wrap items-baseline gap-x-2 text-sm/6">
								<a
									href={resolve(`/projects/${encodeURIComponent(p.id)}` as '/')}
									class="rounded-sm font-semibold text-fg focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
									><span class="absolute inset-0" aria-hidden="true"></span>{p.name}</a
								>
								{#if p.key}<span class="font-mono text-xs/5 text-fg-muted">{p.key}</span>{/if}
							</p>
							<div class="mt-1">
								<div class={factsWrap}>
									<ul role="list" class="{factsList} text-xs/5 text-fg-muted">
										<li class="{fact} font-medium text-fg-secondary">
											{p.role === 'owner' ? 'Owner' : 'Contributor'}
										</li>
										{#if p.ownerTeam}
											<li class={fact}>
												Owned by {@render teamLink(p.ownerTeam)}
											</li>
										{/if}
										{#if p.contributors?.length}
											<li class={fact}>
												With {#each p.contributors as t, i (t.id)}{#if i > 0},
													{/if}{@render teamLink(t)}{/each}
											</li>
										{/if}
										<li class="{fact} tabular-nums">{openWork(p.openWork)}</li>
									</ul>
								</div>
							</div>
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	{/snippet}
</Loaded>
