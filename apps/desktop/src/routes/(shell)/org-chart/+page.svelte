<script lang="ts">
	import { SvelteSet } from 'svelte/reactivity';
	import LayoutList from '@lucide/svelte/icons/layout-list';
	import Network from '@lucide/svelte/icons/network';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import MemberTile from '$lib/components/MemberTile.svelte';
	import OwnerBar from '$lib/components/OwnerBar.svelte';
	import PeopleChart from '$lib/components/PeopleChart.svelte';
	import ReportingChart from '$lib/components/ReportingChart.svelte';
	import TeamsChart from '$lib/components/TeamsChart.svelte';
	import TeamsList from '$lib/components/TeamsList.svelte';
	import {
		highlightedByOffice,
		nonOwners,
		notInATeam,
		organizationCounts,
		owner,
		teamTree,
		unassignedProjects,
		type OfficeFilter,
		type TeamNode
	} from '$lib/org/model';
	import { reportingTree, type ReportNode } from '$lib/org/reporting';

	let { data } = $props();
	const snapshot = $derived(data.snapshot);

	const TABS = [
		{ id: 'teams', label: 'Teams' },
		{ id: 'people', label: 'People' },
		{ id: 'reporting', label: 'Reporting lines' }
	] as const;
	let tab = $state<(typeof TABS)[number]['id']>('teams');
	let asList = $state(false);
	let fit = $state(true);
	const collapsed = new SvelteSet<string>();
	const collapsedReports = new SvelteSet<string>();
	const reporting = $derived(reportingTree(snapshot));
	let selected = $state<string | undefined>();
	let filterValue = $state('all');

	const tree = $derived(teamTree(snapshot));
	const counts = $derived(organizationCounts(snapshot));
	const lone = $derived(notInATeam(snapshot));
	const unassigned = $derived(unassignedProjects(snapshot));
	const offices = $derived(data.connection.organization.offices);
	const filter = $derived<OfficeFilter>(
		filterValue === 'all' ? { kind: 'all' } : filterValue === 'unplaced' ? { kind: 'unplaced' } : { kind: 'office', id: filterValue }
	);

	// What the office filter highlights: the office's name for the tag, and how many agents it covers.
	const highlightLabel = $derived(filter.kind === 'office' ? (offices.find((o) => o.id === filter.id)?.name ?? 'office') : filter.kind === 'unplaced' ? 'Unplaced' : '');
	const highlightedCount = $derived(nonOwners(snapshot).filter((m) => highlightedByOffice(m, filter) === true).length);

	const reportParents = (n: ReportNode): string[] => (n.children.length ? [n.member.id, ...n.children.flatMap(reportParents)] : []).filter((id) => id !== n.member.id || !n.member.owner);
	const parents = (nodes: TeamNode[]): string[] => nodes.flatMap((n) => (n.children.length ? [n.team.id, ...parents(n.children)] : []));

	function onTabKey(e: KeyboardEvent) {
		const i = TABS.findIndex((t) => t.id === tab);
		const next =
			e.key === 'ArrowRight' ? i + 1 : e.key === 'ArrowLeft' ? i - 1 : e.key === 'Home' ? 0 : e.key === 'End' ? TABS.length - 1 : undefined;
		if (next === undefined) return;
		e.preventDefault();
		tab = TABS[(next + TABS.length) % TABS.length].id;
		document.getElementById(`tab-${tab}`)?.focus();
	}
	const select = (id: string) => (selected = selected === id ? undefined : id);
</script>

<svelte:head><title>Org chart · Agentic Org</title></svelte:head>

<div class="space-y-5 px-8 py-6">
	<!-- TEMPORARY: until the real office answers org.snapshot, this page shows an invented organization. -->
	<p class="text-xs/5 text-gray-400" data-testid="sample-data-note">
		<Badge variant="secondary">Sample data</Badge> This chart shows an invented organization until your office provides the real one.
	</p>
	<OwnerBar owner={owner(snapshot)} organization={data.connection.organization.name} />

	<div class="flex flex-wrap items-center justify-between gap-3">
		<div role="tablist" aria-label="Org chart views" class="inline-flex rounded-lg bg-white/5 p-1 outline-1 -outline-offset-1 outline-white/10">
			{#each TABS as t (t.id)}
				<button
					type="button"
					role="tab"
					id="tab-{t.id}"
					aria-selected={tab === t.id}
					aria-controls={tab === t.id ? `panel-${t.id}` : undefined}
					tabindex={tab === t.id ? 0 : -1}
					onclick={() => (tab = t.id)}
					onkeydown={onTabKey}
					class="rounded-md px-3 py-1.5 text-sm/6 font-semibold focus-visible:outline-2 focus-visible:outline-indigo-400 {tab === t.id ? 'bg-white/10 text-white' : 'text-gray-400 hover:text-white'}"
				>
					{t.label}
				</button>
			{/each}
		</div>
		<p class="text-sm/6 text-gray-400">
			<span class="font-semibold text-white">{counts.agents}</span> agents ·
			<span class="font-semibold text-white">{counts.people}</span> people ·
			{#if counts.others > 0}<span class="font-semibold text-white">{counts.others}</span> other ·{/if}
			<span class="font-semibold text-white">{counts.teams}</span> teams
		</p>
	</div>

	{#if tab === 'teams'}
		<div id="panel-teams" role="tabpanel" aria-labelledby="tab-teams" class="space-y-5">
			<h2 class="sr-only">Teams</h2>
			<div class="flex flex-wrap items-center gap-3">
				<label class="flex items-center gap-x-2 text-sm/6 text-gray-300">
					Office
					<select aria-label="Office filter" bind:value={filterValue} class="rounded-md bg-gray-800 px-2 py-1 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
						<option value="all">All offices</option>
						{#each offices as office (office.id)}<option value={office.id}>{office.name} · {office.host}</option>{/each}
						<option value="unplaced">Unplaced</option>
					</select>
				</label>
				<div class="ml-auto flex items-center gap-x-2">
					<Button variant="outline" size="sm" onclick={() => (asList = !asList)}>
						{#if asList}<Network aria-hidden="true" /> Chart{:else}<LayoutList aria-hidden="true" /> List{/if}
					</Button>
					{#if !asList}
						<Button variant="outline" size="sm" onclick={() => (fit = !fit)}>{fit ? 'Actual size' : 'Fit to width'}</Button>
						<Button variant="outline" size="sm" onclick={() => collapsed.clear()}>Expand all</Button>
						<Button variant="outline" size="sm" onclick={() => parents(tree).forEach((id) => collapsed.add(id))}>Collapse all</Button>
					{/if}
				</div>
			</div>

			<!-- A live region that exists before it is filled is announced reliably; the visible line below is separate. -->
			<p class="sr-only" role="status" data-testid="filter-announcement">
				{filter.kind === 'all' ? '' : `Highlighting ${highlightedCount} ${highlightedCount === 1 ? 'agent' : 'agents'} ${filter.kind === 'unplaced' ? 'not placed in an office' : `at ${highlightLabel}`}`}
			</p>
			{#if filter.kind !== 'all'}
				<p class="text-sm/6 text-amber-200" data-testid="filter-status">
					Highlighting {highlightedCount} {highlightedCount === 1 ? 'agent' : 'agents'}
					{filter.kind === 'unplaced' ? 'not placed in an office' : `at ${highlightLabel}`} ·
					<button type="button" class="font-semibold underline underline-offset-2 focus-visible:outline-2 focus-visible:outline-indigo-400" onclick={() => (filterValue = 'all')}>Clear</button>
				</p>
			{/if}

			{#if unassigned.length > 0}
				<p class="text-sm/6 text-amber-200" data-testid="unassigned-note">
					{unassigned.length} {unassigned.length === 1 ? 'project has' : 'projects have'} no team yet.
				</p>
			{/if}

			{#if tree.length === 0}
				<div class="rounded-xl bg-white/5 px-6 py-10 text-center outline-1 -outline-offset-1 outline-white/10">
					<p class="text-sm/6 font-semibold text-white">No teams yet</p>
					<p class="mx-auto mt-1 max-w-sm text-sm/6 text-gray-400">Teams group agents and people, and can own other teams and projects.</p>
				</div>
			{:else if asList}
				<TeamsList {snapshot} {tree} />
			{:else}
				<div class="overflow-x-auto pb-4">
					<TeamsChart {snapshot} {tree} {collapsed} {filter} {selected} {fit} {highlightLabel} onselect={select} />
				</div>
			{/if}

			{#if lone.length > 0}
				<section aria-labelledby="no-team" class="rounded-xl bg-gray-800/50 p-3 outline-1 -outline-offset-1 outline-white/10" data-testid="not-in-a-team">
					<h3 id="no-team" class="text-sm/6 font-semibold text-white">Not in a team <span class="font-normal text-gray-400">{lone.length}</span></h3>
					<div class="mt-2 flex flex-wrap gap-2">
						{#each lone as m (m.id)}
							<MemberTile
								member={m}
								size="sm"
								selected={selected === m.id}
								highlighted={highlightedByOffice(m, filter) === true}
								{highlightLabel}
								dimmed={highlightedByOffice(m, filter) === false}
								onselect={select}
							/>
						{/each}
					</div>
				</section>
			{/if}

			{#if unassigned.length > 0}
				<section aria-labelledby="unassigned" class="rounded-xl bg-gray-800/50 p-3 outline-1 -outline-offset-1 outline-white/10" data-testid="unassigned-projects">
					<h3 id="unassigned" class="text-sm/6 font-semibold text-white">Unassigned projects</h3>
					<ul class="mt-2 space-y-1 text-sm/6 text-gray-300">
						{#each unassigned as project (project.id)}<li>{project.name}</li>{/each}
					</ul>
				</section>
			{/if}
		</div>
	{:else if tab === 'people'}
		<div id="panel-people" role="tabpanel" aria-labelledby="tab-people" class="space-y-5">
			<h2 class="sr-only">People</h2>
			<!-- PEOPLE-2: always said, so nobody expects an agent to reach someone who isn't listed. -->
			<p class="rounded-lg bg-sky-500/10 px-4 py-3 text-sm/6 text-sky-100 outline-1 -outline-offset-1 outline-sky-400/20" data-testid="people-note">
				Agents can only reach the people listed in your organization.
			</p>
			{#if tree.length === 0}
				<div class="rounded-xl bg-white/5 px-6 py-10 text-center outline-1 -outline-offset-1 outline-white/10">
					<p class="text-sm/6 font-semibold text-white">No teams yet</p>
				</div>
			{:else}
				<div class="flex justify-end gap-x-2">
					<Button variant="outline" size="sm" onclick={() => (fit = !fit)}>{fit ? 'Actual size' : 'Fit to width'}</Button>
					<Button variant="outline" size="sm" onclick={() => collapsed.clear()}>Expand all</Button>
					<Button variant="outline" size="sm" onclick={() => parents(tree).forEach((id) => collapsed.add(id))}>Collapse all</Button>
				</div>
				<div class="overflow-x-auto pb-4">
					<PeopleChart {snapshot} {tree} {collapsed} {selected} {fit} onselect={select} />
				</div>
			{/if}
			{#if lone.some((m) => m.kind === 'person')}
				<section aria-labelledby="people-no-team" class="rounded-xl bg-gray-800/50 p-3 outline-1 -outline-offset-1 outline-white/10" data-testid="people-not-in-a-team">
					<h3 id="people-no-team" class="text-sm/6 font-semibold text-white">Not in a team <span class="font-normal text-gray-400">{lone.filter((m) => m.kind === 'person').length}</span></h3>
					<div class="mt-2 flex flex-wrap gap-2">
						{#each lone.filter((m) => m.kind === 'person') as m (m.id)}
							<MemberTile member={m} size="sm" selected={selected === m.id} onselect={select} />
						{/each}
					</div>
				</section>
			{/if}
		</div>
	{:else}
		<div id="panel-reporting" role="tabpanel" aria-labelledby="tab-reporting" class="space-y-5">
			<h2 class="sr-only">Reporting lines</h2>
			<!-- REPORT-5: reporting lines and teams are separate things. -->
			<p class="rounded-lg bg-sky-500/10 px-4 py-3 text-sm/6 text-sky-100 outline-1 -outline-offset-1 outline-sky-400/20" data-testid="reporting-note">
				Reporting lines show who reports to whom. They are separate from teams: changing a team or its lead doesn't change anyone's manager.
			</p>
			{#if reporting}
				<div class="flex justify-end gap-x-2">
					<Button variant="outline" size="sm" onclick={() => (fit = !fit)}>{fit ? 'Actual size' : 'Fit to width'}</Button>
					<Button variant="outline" size="sm" onclick={() => collapsedReports.clear()}>Expand all</Button>
					<Button variant="outline" size="sm" onclick={() => reportParents(reporting).forEach((id) => collapsedReports.add(id))}>Collapse all</Button>
				</div>
				<div class="overflow-x-auto pb-4">
					<ReportingChart {snapshot} root={reporting} collapsed={collapsedReports} {selected} {fit} onselect={select} />
				</div>
			{/if}
		</div>
	{/if}
</div>
