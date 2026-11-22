<!--
  @component
  A project's Team tab (work-items-ux §3, PRJ-2, TEAM-13): the accountable team, then the contributing teams, each a
  link with its reference and its lead when it has one (a lead of a participating team is not the project's lead);
  then the members whose home team is one of those teams, A–Z, with the word Agent or Person. Who is on which team is
  changed on the teams themselves; which teams take part is changed in Settings.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { kindWord } from '$lib/modules/members';
	import { projectsApi } from '../../api';
	import { projectHref } from '../../present';
	import { inlineLink, rowTitleLink } from '../../components/classes';
	import type { ProjectTeamEntry } from '../../types';
	import { projectFrame } from '../frame/context';

	const frame = projectFrame();
	const view = new Resource(() => projectsApi.team(frame.organizationId, frame.projectId));

	const gutters = 'px-4 min-[640px]:px-6 min-[1024px]:px-8';
	const h4 = 'text-sm/6 font-semibold text-fg';
</script>

{#snippet teamRow(t: ProjectTeamEntry, role: 'Accountable' | 'Contributor')}
	<li
		class="relative flex flex-wrap items-baseline gap-x-4 gap-y-1 py-4 {gutters}"
		data-team={t.id}
	>
		<p class="min-w-0 flex-1 basis-60 text-sm/6">
			<a href={resolve(`/teams/${encodeURIComponent(t.id)}` as '/')} class={rowTitleLink}
				><span class="absolute inset-0" aria-hidden="true"></span>{t.name}</a
			>
			{#if t.ref}<span class="ml-2 font-mono text-xs/5 text-fg-muted">{t.ref}</span>{/if}
		</p>
		<p class="text-xs/5 text-fg-muted">
			<span class="font-medium text-fg-secondary">{role}</span>
			· {t.lead ? `Led by ${t.lead.name}` : 'No lead'}
		</p>
	</li>
{/snippet}

<Loaded
	resource={view}
	label="the project's team"
	error="Couldn't load the project's team"
	errorLevel={4}
	avatar
>
	{#snippet children(v)}
		<section aria-labelledby="teams-heading" class="pt-6">
			<div class="flex flex-wrap items-baseline justify-between gap-x-4 {gutters}">
				<h4 id="teams-heading" class={h4}>Teams</h4>
				{#if frame.project.viewer.canEdit}
					<a
						href={resolve(projectHref(frame.projectId, 'settings') as '/')}
						class="{inlineLink.sm} text-sm/6">Change teams in Settings</a
					>
				{/if}
			</div>
			<ul role="list" class="mt-2 divide-y divide-line-subtle border-y border-line-subtle">
				{@render teamRow(v.owner, 'Accountable')}
				{#each v.contributors as t (t.id)}{@render teamRow(t, 'Contributor')}{/each}
			</ul>
			{#if v.contributors.length === 0}
				<p class="py-3 text-xs/5 text-fg-muted {gutters}">No contributing teams.</p>
			{/if}
		</section>

		<section aria-labelledby="people-heading" class="pt-6">
			<h4 id="people-heading" class="{h4} {gutters}">
				Members <span class="font-normal text-fg-muted tabular-nums">{v.members.length}</span>
			</h4>
			{#if v.members.length === 0}
				<p class="py-4 text-sm/6 text-fg-muted {gutters}">
					Nobody's home team is one of these teams yet. People and agents join a team from its page.
				</p>
			{:else}
				<ul role="list" class="mt-2 divide-y divide-line-subtle border-t border-line-subtle">
					{#each v.members as m (m.id)}
						<li class="relative flex items-center gap-x-4 py-4 {gutters}">
							<Avatar name={m.name} />
							<div class="min-w-0 flex-1">
								<p class="text-sm/6">
									<a
										href={resolve(`/members/${encodeURIComponent(m.id)}` as '/')}
										class={rowTitleLink}
										><span class="absolute inset-0" aria-hidden="true"></span>{m.name}</a
									>
								</p>
								<p class="mt-1 text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
									{kindWord(m.kind)}{#if m.role}
										· {m.role}{/if} · {m.homeTeam.name}
								</p>
							</div>
						</li>
					{/each}
				</ul>
			{/if}
		</section>
	{/snippet}
</Loaded>
