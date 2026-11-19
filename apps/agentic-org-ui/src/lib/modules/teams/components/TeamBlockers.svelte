<!--
  @component
  What stops a team from being archived or deleted (offices-and-teams-ux §7.10, TEAM-9), each with a link to where it
  is resolved: a team under it to its page (move or archive it), an employee to their profile (Move home team), a
  project to its page (choose another owner team). Open tasks are listed without links until the work item page can
  open a task. `onleave` closes the dialog around it when a link is followed.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { kindWord } from '$lib/modules/members';
	import type { TeamBlockers } from '../blockers';
	import { teamHref } from '../team-page';

	let { name, blockers, onleave }: { name: string; blockers: TeamBlockers; onleave: () => void } =
		$props();

	const link =
		'inline-flex min-h-6 items-center rounded-sm font-medium text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 [overflow-wrap:anywhere]';
	const heading = 'font-semibold text-fg';
</script>

<div class="space-y-4 text-sm/6" data-team-blockers>
	{#if blockers.teams.length > 0}
		<section aria-labelledby="team-blocking-teams">
			<h3 id="team-blocking-teams" class={heading}>Teams under {name}</h3>
			<ul role="list" class="mt-1 space-y-1">
				{#each blockers.teams as t (t.id)}
					<li>
						<a href={resolve(teamHref(t.id) as '/')} class={link} onclick={onleave}>{t.name}</a
						>{#if t.ref}<span class="ml-2 font-mono text-xs/5 text-fg-muted">{t.ref}</span>{/if}
					</li>
				{/each}
			</ul>
		</section>
	{/if}
	{#if blockers.employees.length > 0}
		<section aria-labelledby="team-blocking-employees">
			<h3 id="team-blocking-employees" class={heading}>Home team of</h3>
			<ul role="list" class="mt-1 space-y-1">
				{#each blockers.employees as m (m.id)}
					<li>
						<a
							href={resolve(`/members/${encodeURIComponent(m.id)}` as '/')}
							class={link}
							onclick={onleave}>{m.name}</a
						><span class="ml-2 text-xs/5 text-fg-muted">{kindWord(m.kind)}</span>
					</li>
				{/each}
			</ul>
		</section>
	{/if}
	{#if blockers.projects.length > 0}
		<section aria-labelledby="team-blocking-projects">
			<h3 id="team-blocking-projects" class={heading}>Projects {name} owns</h3>
			<ul role="list" class="mt-1 space-y-1">
				{#each blockers.projects as p (p.id)}
					<li>
						<a
							href={resolve(`/projects/${encodeURIComponent(p.id)}` as '/')}
							class={link}
							onclick={onleave}>{p.name}</a
						>
					</li>
				{/each}
			</ul>
		</section>
	{/if}
	{#if blockers.tasks.length > 0}
		<section aria-labelledby="team-blocking-tasks">
			<h3 id="team-blocking-tasks" class={heading}>Open tasks</h3>
			<ul role="list" class="mt-1 space-y-1 text-fg-secondary">
				{#each blockers.tasks as t (t.ref)}
					<li class="[overflow-wrap:anywhere]">
						<span class="font-mono text-xs/5 text-fg-muted">{t.ref}</span>
						{t.title}
					</li>
				{/each}
			</ul>
		</section>
	{/if}
</div>
