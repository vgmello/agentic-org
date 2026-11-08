<script lang="ts">
	import './org-tree.css';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import { SvelteSet } from 'svelte/reactivity';
	import MemberTile from './MemberTile.svelte';
	import {
		agentsOf,
		highlightedByOffice,
		leadOf,
		membersOfTeam,
		othersOf,
		otherTeamsOf,
		otherTeamsOnProject,
		peopleOf,
		projectsOfTeam,
		subTeamCount,
		type OfficeFilter,
		type TeamNode
	} from '$lib/org/model';
	import type { Member, OrgSnapshot } from '$lib/proto';

	let {
		snapshot,
		tree,
		collapsed,
		filter,
		selected,
		onselect,
		fit = true,
		highlightLabel = 'here'
	}: {
		snapshot: OrgSnapshot;
		tree: TeamNode[];
		collapsed: SvelteSet<string>;
		filter: OfficeFilter;
		selected: string | undefined;
		onselect: (id: string) => void;
		/** Shrink the chart (never below 45%) to the width available, instead of scrolling. */
		fit?: boolean;
		/** What the office-filter tag says: the filtered office's name. */
		highlightLabel?: string;
	} = $props();

	let chart = $state<HTMLElement>();
	let available = $state(0);
	let natural = $state({ w: 0, h: 0 });
	const scale = $derived(fit && natural.w > 0 && available > 0 ? Math.min(1, Math.max(0.45, available / natural.w)) : 1);

	$effect(() => {
		if (!chart) return;
		// Apply the new size on the next frame, not inside the observer callback: changing layout there
		// is what makes browsers log "ResizeObserver loop completed with undelivered notifications".
		let frame = 0;
		const watch = new ResizeObserver(([entry]) => {
			cancelAnimationFrame(frame);
			frame = requestAnimationFrame(() => (natural = { w: entry.contentRect.width, h: entry.contentRect.height }));
		});
		watch.observe(chart);
		return () => {
			cancelAnimationFrame(frame);
			watch.disconnect();
		};
	});

	const tileProps = (m: Member, teamId: string) => ({
		member: m,
		size: 'sm' as const,
		selected: selected === m.id,
		highlighted: highlightedByOffice(m, filter) === true,
		highlightLabel,
		dimmed: highlightedByOffice(m, filter) === false,
		alsoIn: otherTeamsOf(snapshot, m.id, teamId),
		onselect
	});

	// Which shared-project popover is open (one at a time).
	let sharedOpen = $state<string | undefined>();

	function toggle(id: string) {
		if (collapsed.has(id)) collapsed.delete(id);
		else collapsed.add(id);
	}
</script>

{#snippet branch(node: TeamNode)}
	{@const team = node.team}
	{@const members = membersOfTeam(snapshot, team.id)}
	{@const agents = agentsOf(members)}
	{@const people = peopleOf(members)}
	{@const others = othersOf(members)}
	{@const lead = leadOf(snapshot, team)}
	{@const projects = projectsOfTeam(snapshot, team.id)}
	{@const open = !collapsed.has(team.id)}
	<div class="flex w-64 flex-col items-center" data-testid="team-card" data-team={team.id}>
		<div class="w-full rounded-xl bg-gray-800/50 p-3 text-left outline-1 -outline-offset-1 outline-white/10">
			<h3 class="truncate text-sm/6 font-semibold text-white" title={team.name}>{team.name}</h3>
			<p class="text-xs/5 text-gray-400">
				{#if lead}Lead: <span class="break-words text-gray-200">{lead.name}</span>{:else}No lead{/if}
			</p>

			<dl class="mt-2 space-y-1.5 text-xs/5">
				<div class="flex items-center gap-x-2">
					<dt class="w-14 shrink-0 text-gray-400">Agents <span class="text-gray-300">{agents.length}</span></dt>
					<dd class="flex flex-wrap gap-1.5">
						{#each agents as m (m.id)}<MemberTile {...tileProps(m, team.id)} />{:else}<span class="text-gray-500">None</span>{/each}
					</dd>
				</div>
				<div class="flex items-center gap-x-2">
					<dt class="w-14 shrink-0 text-gray-400">People <span class="text-gray-300">{people.length}</span></dt>
					<dd class="flex flex-wrap gap-1.5">
						{#each people as m (m.id)}<MemberTile {...tileProps(m, team.id)} />{:else}<span class="text-gray-500">None</span>{/each}
					</dd>
				</div>
				{#if others.length > 0}
					<div class="flex items-center gap-x-2">
						<dt class="w-14 shrink-0 text-gray-400">Other <span class="text-gray-300">{others.length}</span></dt>
						<dd class="flex flex-wrap gap-1.5">
							{#each others as m (m.id)}<MemberTile {...tileProps(m, team.id)} />{/each}
						</dd>
					</div>
				{/if}
			</dl>

			{#if projects.length > 0}
				<ul class="mt-2 space-y-1 border-t border-white/5 pt-2 text-xs/5" aria-label="Projects of {team.name}">
					{#each projects as project (project.id)}
						{@const others = otherTeamsOnProject(snapshot, project.id, team.id)}
						<li class="flex flex-wrap items-center gap-x-2">
							<span class="font-medium break-words text-gray-200">{project.name}</span>
							<span class="text-gray-400">· {lead ? `lead ${lead.name}` : 'No lead'}</span>
							{#if others.length > 0}
								<button
									type="button"
									aria-expanded={sharedOpen === `${team.id}|${project.id}`}
									aria-label="Shared project {project.name}, also worked on by {others.map((t) => t.name).join(', ')}"
									onclick={() => (sharedOpen = sharedOpen === `${team.id}|${project.id}` ? undefined : `${team.id}|${project.id}`)}
									onkeydown={(e) => e.key === 'Escape' && (sharedOpen = undefined)}
									class="rounded bg-sky-500/15 px-1.5 text-[0.65rem] leading-4 font-semibold text-sky-200 outline-1 outline-sky-400/30 focus-visible:outline-2 focus-visible:outline-indigo-400"
								>
									Shared
								</button>
								{#if sharedOpen === `${team.id}|${project.id}`}
									<span class="basis-full text-gray-400">Also: {others.map((t) => t.name).join(', ')}</span>
								{/if}
							{/if}
						</li>
					{/each}
				</ul>
			{/if}
		</div>

		{#if node.children.length > 0}
			<button
				type="button"
				onclick={() => toggle(team.id)}
				aria-expanded={open}
				aria-label="{subTeamCount(node)} {subTeamCount(node) === 1 ? 'team' : 'teams'}: {open ? 'collapse' : 'expand'} the teams under {team.name}"
				class="-mt-px flex h-6 items-center gap-x-1 rounded-b-md bg-gray-800/70 px-2 text-xs text-gray-300 outline-1 -outline-offset-1 outline-white/10 hover:bg-white/10 hover:text-white focus-visible:outline-2 focus-visible:outline-indigo-500"
			>
				{subTeamCount(node)} {subTeamCount(node) === 1 ? 'team' : 'teams'}
				<ChevronDown class="size-3.5 transition-transform {open ? '' : '-rotate-90'}" aria-hidden="true" />
			</button>
		{/if}
	</div>
	{#if node.children.length > 0 && open}
		<ul class="t">
			{#each node.children as child (child.team.id)}<li class="t">{@render branch(child)}</li>{/each}
		</ul>
	{/if}
{/snippet}

<div bind:clientWidth={available} class="w-full">
	<div class="mx-auto" style="width: {natural.w * scale}px; height: {natural.h * scale}px;">
		<div bind:this={chart} class="org-tree" data-testid="teams-chart" style="transform: scale({scale}); transform-origin: top left;">
			<ul class="t">
				{#each tree as node (node.team.id)}<li class="t">{@render branch(node)}</li>{/each}
			</ul>
		</div>
	</div>
</div>
