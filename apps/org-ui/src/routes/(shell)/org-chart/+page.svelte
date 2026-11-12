<script lang="ts">
	import { AgentsTabs } from '$lib/modules/employees';
	import { invalidate } from '$app/navigation';
	import { page } from '$app/state';
	import { setContext } from 'svelte';
	import { MediaQuery, SvelteSet } from 'svelte/reactivity';
	import { toast } from 'svelte-sonner';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator } from '$lib/components/ui/dropdown-menu';
	import Callout from '$lib/components/Callout.svelte';
	import ChartFitMenuItem from '$lib/components/ChartFitMenuItem.svelte';
	import ProjectTeamDialog from '$lib/components/ProjectTeamDialog.svelte';
	import RemoveProjectDialog from '$lib/components/RemoveProjectDialog.svelte';
	import MoveMemberDialog from '$lib/components/MoveMemberDialog.svelte';
	import { AddAgentSheet, EditAgentDialog, HowItRunsDialog, OffboardAgentDialog, PauseAgentDialog } from '$lib/modules/employees';
	import AddPersonSheet from '$lib/components/AddPersonSheet.svelte';
	import EditPersonDialog from '$lib/components/EditPersonDialog.svelte';
	import RemovePersonDialog from '$lib/components/RemovePersonDialog.svelte';
	import ChangeManagerDialog from '$lib/components/ChangeManagerDialog.svelte';
	import EditMemberTeamsDialog from '$lib/components/EditMemberTeamsDialog.svelte';
	import EditMembersDialog from '$lib/components/EditMembersDialog.svelte';
	import DeleteTeamDialog from '$lib/components/DeleteTeamDialog.svelte';
	import MemberTile from '$lib/components/MemberTile.svelte';
	import MoveTeamDialog from '$lib/components/MoveTeamDialog.svelte';
	import NewTeamSheet from '$lib/components/NewTeamSheet.svelte';
	import OwnerBar from '$lib/components/OwnerBar.svelte';
	import PeopleChart from '$lib/components/PeopleChart.svelte';
	import RenameTeamDialog from '$lib/components/RenameTeamDialog.svelte';
	import ReportingChart from '$lib/components/ReportingChart.svelte';
	import SelectedMember from '$lib/components/SelectedMember.svelte';
	import TeamMenu, { type TeamAction } from '$lib/components/TeamMenu.svelte';
	import TeamsChart from '$lib/components/TeamsChart.svelte';
	import TeamsList from '$lib/components/TeamsList.svelte';
	import {
		formerMembers,
		highlightedByOffice,
		nonOwners,
		memberById,
		notInATeam,
		organizationCounts,
		owner,
		searchMembers,
		teamTree,
		unassignedProjects,
		withoutFormer,
		withoutPaused,
		type OfficeFilter,
		type TeamNode
	} from '$lib/org/model';
	import { toAppError } from '$lib/office/api';
	import { orgApi } from '$lib/org/api';
	import { reportingTree, type ReportNode } from '$lib/org/reporting';
	import { sessionFor } from '$lib/org/session';
	import { removeProjectTeam, setAgentStatus } from '$lib/org/actions';
	import { isLastTeam } from '$lib/org/team-edit';
	import type { Member, Team } from '$lib/proto';

	let { data } = $props();
	// Everyone, including agents who have left (their names stay reserved), and the organization as the chart shows it.
	const full = $derived(data.snapshot);
	const live = $derived(withoutFormer(full));
	// What the charts draw: everyone who is here, except paused agents while "Show paused" is off (DEAL-5).
	let showPaused = $state(true);
	const snapshot = $derived(showPaused ? live : withoutPaused(live));
	const anyPaused = $derived(live.members.some((m) => m.agent?.status === 'paused'));
	// With nothing paused the toggle is hidden: it must not stay off, or the next agent paused would vanish at once.
	$effect(() => {
		if (!anyPaused) showPaused = true;
	});
	const former = $derived(formerMembers(full));

	const TABS = [
		{ id: 'teams', label: 'Teams' },
		{ id: 'people', label: 'People' },
		{ id: 'reporting', label: 'Reporting lines' }
	] as const;
	let tab = $state<(typeof TABS)[number]['id']>('teams');
	const narrow = new MediaQuery('(width < 640px)');
	let chosen = $state<'list' | 'chart' | null>(null);
	// Responsive defaults follow the viewport; an explicit choice lasts for this page visit.
	const asList = $derived((chosen ?? (narrow.current ? 'list' : 'chart')) === 'list');
	let fit = $state(true);
	let fitBlocked = $state(false);
	let viewMenuOpen = $state(false);
	const onPeopleFitBlocked = (blocked: boolean) => (fitBlocked = blocked);
	const collapsed = new SvelteSet<string>();
	const collapsedReports = new SvelteSet<string>();
	const reporting = $derived(reportingTree(snapshot));
	let selected = $state<string | undefined>(page.url.searchParams.get('member') ?? undefined); // from the Agents screen
	// The search is kept when the tab changes (TEAM-11), and read by every tile through a context.
	let query = $state('');
	let searchInput = $state<HTMLInputElement>();
	const matches = $derived(searchMembers(snapshot, query));
	setContext('member-search', {
		get matches() {
			return matches;
		}
	});
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
	const highlightedCount = $derived(nonOwners(snapshot).filter((m) => highlightedByOffice(m, filter, knownOffices) === true).length);

	const reportParents = (n: ReportNode): string[] => (n.children.length ? [n.member.id, ...n.children.flatMap(reportParents)] : []).filter((id) => id !== n.member.id || !n.member.owner);
	const knownOffices = $derived(new Set(offices.map((o) => o.id)));
	const selectedMember = $derived(memberById(snapshot, selected));
	const parents = (nodes: TeamNode[]): string[] => nodes.flatMap((n) => (n.children.length ? [n.team.id, ...parents(n.children)] : []));

	/**
	 * Arrow keys move between the member tiles in the chart you're in (TEAM-10), in reading order; Home and End go
	 * to the first and last. Tab still visits every control (menus, toggles) as before.
	 */
	function onChartFrameKey(e: KeyboardEvent & { currentTarget: EventTarget & HTMLDivElement }) {
		if (e.target !== e.currentTarget || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || !['ArrowLeft', 'ArrowRight'].includes(e.key)) return;
		e.preventDefault();
		e.currentTarget.scrollBy({ left: e.key === 'ArrowRight' ? 40 : -40, behavior: 'auto' });
	}

	function onTileKey(e: KeyboardEvent) {
		const here = (e.target as HTMLElement).closest('[data-member-tile]');
		if (!here || !['ArrowRight', 'ArrowDown', 'ArrowLeft', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
		const tiles = Array.from((e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('[data-member-tile]'));
		const i = tiles.indexOf(here as HTMLElement);
		const next = e.key === 'Home' ? 0 : e.key === 'End' ? tiles.length - 1 : e.key === 'ArrowRight' || e.key === 'ArrowDown' ? Math.min(i + 1, tiles.length - 1) : Math.max(i - 1, 0);
		e.preventDefault();
		tiles[next]?.focus();
	}

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

	// ---- editing ----
	const session = $derived(sessionFor(orgApi, data.connection.organization.id));
	const reload = () => invalidate('app:org');
	let newTeamOpen = $state(false);
	let newTeamUnder = $state<string | undefined>();
	let renaming = $state<Team | undefined>();
	let renameOpen = $state(false);
	let deleting = $state<Team | undefined>();
	let deleteOpen = $state(false);
	let editingMembers = $state<Team | undefined>();
	let membersOpen = $state(false);
	let projectTeam = $state<Team | undefined>();
	let assigning = $state<{ id: string; name: string } | undefined>();
	let projectTeamOpen = $state(false);
	let removeProject = $state<{ projectId: string; projectName: string; teamName: string; teamId: string } | undefined>();
	let removeProjectOpen = $state(false);
	let addPersonOpen = $state(false);
	let addPersonTeam = $state<string | undefined>();
	let editingPerson = $state<Member | undefined>();
	let editPersonOpen = $state(false);
	let removingPerson = $state<Member | undefined>();
	let removePersonOpen = $state(false);
	let managing = $state<Member | undefined>();
	let managerOpen = $state(false);
	let memberTeams = $state<Member | undefined>();
	let memberTeamsOpen = $state(false);
	let movingMember = $state<Member | undefined>();
	let moveMemberOpen = $state(false);
	let addAgentOpen = $state(false);
	let pausing = $state<Member | undefined>();
	let pauseOpen = $state(false);
	let offboarding = $state<Member | undefined>();
	let offboardOpen = $state(false);
	let editingAgent = $state<Member | undefined>();
	let editAgentOpen = $state(false);
	let howRuns = $state<Member | undefined>();
	let howRunsOpen = $state(false);
	let moving = $state<Team | undefined>();
	let moveOpen = $state(false);
	let justAdded = $state<string | undefined>();

	function teamAction(action: TeamAction, team: Team) {
		if (action === 'rename') ((renaming = team), (renameOpen = true));
		else if (action === 'delete') ((deleting = team), (deleteOpen = true));
		else if (action === 'move') ((moving = team), (moveOpen = true));
		else if (action === 'members') ((editingMembers = team), (membersOpen = true));
		else if (action === 'add-person') ((addPersonTeam = team.id), (addPersonOpen = true));
		else if (action === 'add-project') ((projectTeam = team), (assigning = undefined), (projectTeamOpen = true));
		else ((newTeamUnder = team.id), (newTeamOpen = true));
	}
	function newTeam(under?: string) {
		newTeamUnder = under;
		newTeamOpen = true;
	}
	function flash(teamId: string) {
		justAdded = teamId;
		setTimeout(() => justAdded === teamId && (justAdded = undefined), 2000);
	}
	async function teamCreated(r: { teamId: string; name: string; problems: string[] }) {
		await reload();
		flash(r.teamId);
		if (r.problems.length === 0) toast.success(`Created ${r.name}.`);
		else toast.warning(`Created ${r.name}, but not everything went through.`, { description: r.problems.join(' ') });
	}
	async function teamRenamed(r: { name?: string; stale?: boolean }) {
		await reload();
		if (r.name) toast.success(`Renamed to ${r.name}.`);
	}
	async function membersEdited(r: { name: string; changed: boolean; problems: string[] }) {
		await reload();
		if (r.problems.length === 0 && r.changed) toast.success(`Updated the members of ${r.name}.`);
	}
	async function projectGiven(r: { message?: string }) {
		await reload();
		if (r.message) toast.success(r.message);
	}
	/** The × on a project's row: no confirmation while other teams remain; the last one asks first (TEAM-8). */
	async function takeProjectOff(projectId: string, team: Team) {
		const project = (snapshot.projects ?? []).find((p) => p.id === projectId);
		if (!project) return;
		if (isLastTeam(snapshot, projectId, team.id)) {
			removeProject = { projectId, projectName: project.name, teamName: team.name, teamId: team.id };
			removeProjectOpen = true;
			return;
		}
		try {
			await removeProjectTeam(session, projectId, team.id);
			toast.success(`Removed ${project.name} from ${team.name}.`);
		} catch (e) {
			toast.error(`Couldn't remove ${project.name} from ${team.name}: ${toAppError(e).message} Nothing changed.`);
		}
		await reload();
	}
	const organizationName = $derived(data.connection.organization.name);
	async function personAdded(r: { memberId: string; name: string; problems: string[] }) {
		await reload();
		selected = r.memberId;
		if (r.problems.length === 0) toast.success(`Added ${r.name}. Agents can now reach ${r.name}.`);
		else toast.warning(`Added ${r.name}, but not everything went through.`, { description: r.problems.join(' ') });
	}
	async function personEdited(r: { name: string; stale?: boolean }) {
		await reload();
		if (!r.stale) toast.success(`Saved ${r.name}.`);
	}
	async function personRemoved(r: { name: string } | { stale: true }) {
		await reload();
		if ('name' in r) {
			selected = undefined;
			toast.success(`Removed ${r.name}.`);
		}
	}
	async function managerChanged(r: { name: string; to: string; undo: () => Promise<void> } | { stale: true }) {
		await reload();
		if ('stale' in r) return;
		// Undo is the reverse change, offered for 8 seconds (REPORT-4).
		toast.success(`${r.name} now reports to ${r.to}.`, {
			duration: 8000,
			action: {
				label: 'Undo',
				onClick: async () => {
					try {
						await r.undo();
						await reload();
						toast.success(`${r.name}'s manager is back as it was.`);
					} catch (e) {
						await reload();
						toast.error(`Couldn't undo: ${toAppError(e).message} Nothing changed.`);
					}
				}
			}
		});
	}
	async function memberTeamsEdited(r: { name: string; changed: boolean; problems: string[] }) {
		await reload();
		if (r.problems.length === 0 && r.changed) toast.success(`Updated the teams of ${r.name}.`);
	}
	async function memberMoved(r: { name: string; from: string; to: string; lostLead: boolean; undo: () => Promise<void> } | { stale: true }) {
		await reload();
		if ('stale' in r) return;
		toast.success(`${r.name} moved to ${r.to}.`, {
			duration: 8000,
			action: {
				label: 'Undo',
				onClick: async () => {
					try {
						await r.undo();
						toast.success(`${r.name} is back in ${r.from}.${r.lostLead ? ` ${r.name} doesn't lead it again: set the lead if you want that.` : ''}`);
					} catch (e) {
						toast.error(`Couldn't undo: ${toAppError(e).message} Nothing changed.`);
					}
					await reload();
				}
			}
		});
	}
	async function agentAdded(r: { memberId: string; name: string; problems: string[] }) {
		await reload();
		selected = r.memberId;
		if (r.problems.length === 0) toast.success(`Added ${r.name}. Everyone in ${organizationName} can reach ${r.name}.`);
		else toast.warning(`Added ${r.name}, but not everything went through.`, { description: r.problems.join(' ') });
	}
	async function agentPaused(r: { name: string } | { stale: true }) {
		await reload();
		if ('name' in r) toast.success(`Paused ${r.name}. Messages and tasks wait until you resume.`);
	}
	async function resumeAgent(m: Member) {
		try {
			await setAgentStatus(session, m.id, 'active');
			toast.success(`Resumed ${m.name}.`);
		} catch (e) {
			toast.error(`Couldn't resume ${m.name}: ${toAppError(e).message}`);
		}
		await reload();
	}
	async function agentOffboarded(r: { name: string } | { stale: true }) {
		await reload();
		if ('name' in r) {
			selected = undefined;
			toast.success(`Offboarded ${r.name}. The record and name stay.`);
		}
	}
	async function agentEdited(r: { name: string; stale?: boolean }) {
		await reload();
		if (!r.stale) toast.success(`Saved ${r.name}.`);
	}
	async function teamMoved(r: { name: string; to: string; undo: () => Promise<void> } | { stale: true }) {
		await reload();
		if ('stale' in r) return;
		// Undo is the reverse move, offered for 8 seconds (TEAM-7).
		toast.success(`Moved ${r.name} to ${r.to}.`, {
			duration: 8000,
			action: {
				label: 'Undo',
				onClick: async () => {
					try {
						await r.undo();
						await reload();
						toast.success(`Moved ${r.name} back.`);
					} catch (e) {
						await reload();
						toast.error(`Couldn't undo: ${toAppError(e).message} Nothing changed.`);
					}
				}
			}
		});
	}
	async function teamDeleted(r: { name: string } | { stale: true }) {
		await reload();
		if ('name' in r) toast.success(`Deleted ${r.name}.`);
	}
</script>

<svelte:head><title>Org chart · Agentic Org</title></svelte:head>

{#snippet teamMenu(team: Team)}<TeamMenu {team} onaction={teamAction} />{/snippet}

{#snippet plus()}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="M12 4.5v15m7.5-7.5h-15" /></svg>{/snippet}

{#snippet viewMenu()}
	<DropdownMenu bind:open={viewMenuOpen}>
		<DropdownMenuTrigger>
			{#snippet child({ props })}
				<Button {...props} variant="outline" size="sm">View <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="m19.5 8.25-7.5 7.5-7.5-7.5" /></svg></Button>
			{/snippet}
		</DropdownMenuTrigger>
		<!-- Closed content remains mounted for its exit animation; reject stale focus into it. -->
		<DropdownMenuContent inert={!viewMenuOpen} align="end" class="w-48 max-w-[calc(100vw-2rem)]">
			{#if tab === 'teams'}
				<DropdownMenuItem onSelect={() => (chosen = asList ? 'chart' : 'list')}>{asList ? 'Chart' : 'List'}</DropdownMenuItem>
			{/if}
			{#if tab !== 'teams' || !asList}
				{#if tab === 'teams'}<DropdownMenuSeparator />{/if}
				<ChartFitMenuItem {fit} unavailable={tab === 'people' && fitBlocked} ontoggle={() => (fit = !fit)} />
				<DropdownMenuItem onSelect={() => tab === 'reporting' ? collapsedReports.clear() : collapsed.clear()}>Expand all</DropdownMenuItem>
				<DropdownMenuItem onSelect={() => tab === 'reporting' ? reportParents(reporting!).forEach((id) => collapsedReports.add(id)) : parents(tree).forEach((id) => collapsed.add(id))}>Collapse all</DropdownMenuItem>
			{/if}
		</DropdownMenuContent>
	</DropdownMenu>
{/snippet}

<AgentsTabs current="org-chart" />

<div class="space-y-2 px-4 py-4 sm:space-y-5 sm:px-6 sm:py-6 lg:px-8">
	<OwnerBar owner={owner(snapshot)} organization={organizationName} sample={data.sample} />

	<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
		<label class="flex min-w-0 max-w-full flex-wrap items-center gap-x-2 gap-y-1 text-sm/6 text-gray-300 max-[640px]:w-full">
			Search
			<input bind:this={searchInput} type="search" bind:value={query} onkeydown={(e) => e.key === 'Escape' && query !== '' && ((query = ''), e.preventDefault())} placeholder="Search members…" aria-label="Search members" autocomplete="off" class="w-64 min-w-0 max-w-full rounded-md bg-gray-800 px-2 py-1 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 placeholder:text-gray-500 focus-visible:outline-2 focus-visible:outline-indigo-400 max-[640px]:basis-full" />
		</label>
		{#if anyPaused}
			<label class="flex items-center gap-x-2 text-sm/6 text-gray-300"><input type="checkbox" bind:checked={showPaused} /> Show paused</label>
		{/if}
		<p class="text-sm/6 text-sky-200" role="status" data-testid="search-status">
			{#if matches}{matches.size === 0 ? 'No one matches.' : `${matches.size} ${matches.size === 1 ? 'match' : 'matches'}`} · <button type="button" class="font-semibold underline underline-offset-2 focus-visible:outline-2 focus-visible:outline-indigo-400" onclick={() => ((query = ''), searchInput?.focus())}>Clear</button>{/if}
		</p>
	</div>

	<div class="flex flex-wrap items-center justify-between gap-3">
		<div role="tablist" aria-label="Org chart views" class="inline-flex max-w-full flex-wrap rounded-lg bg-white/5 p-1 outline-1 -outline-offset-1 outline-white/10">
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
		{#if data.sample}
			<!-- TEMPORARY: until the real office answers org.snapshot, this page shows an invented organization. -->
			<p class="flex max-w-full flex-wrap items-center gap-x-2 gap-y-1 text-xs/5 text-gray-400" data-testid="sample-data-note">
				<Badge variant="secondary">Sample data</Badge> An invented organization, not yours.
			</p>
		{/if}
		<p class="text-sm/6 text-gray-400">
			<span class="font-semibold text-white">{counts.agents}</span> agents ·
			<span class="font-semibold text-white">{counts.people}</span> people ·
			{#if counts.others > 0}<span class="font-semibold text-white">{counts.others}</span> other ·{/if}
			<span class="font-semibold text-white">{counts.teams}</span> teams
		</p>
	</div>

	<SelectedMember snapshot={live} member={selectedMember} offices={offices} organization={organizationName} onclear={() => (selected = undefined)} onedit={(m) => ((editingPerson = m), (editPersonOpen = true))} onremove={(m) => ((removingPerson = m), (removePersonOpen = true))} onmanager={(m) => ((managing = m), (managerOpen = true))} onteams={(m) => ((memberTeams = m), (memberTeamsOpen = true))} oneditagent={(m) => ((editingAgent = m), (editAgentOpen = true))} onpause={(m) => ((pausing = m), (pauseOpen = true))} onresume={resumeAgent} onhowitruns={(m) => ((howRuns = m), (howRunsOpen = true))} onoffboard={(m) => ((offboarding = m), (offboardOpen = true))} onmove={(m) => ((movingMember = m), (moveMemberOpen = true))} />

	<!-- Arrow keys between tiles are a convenience on top of Tab: the tabpanels themselves stay plain. -->
	<div role="presentation" onkeydown={onTileKey}>
	{#if tab === 'teams'}
		<div id="panel-teams" role="tabpanel" aria-labelledby="tab-teams" class="space-y-5">
			<h2 class="sr-only">Teams</h2>
			<div class="flex flex-wrap items-center gap-3">
				<label class="flex min-w-0 max-w-full flex-wrap items-center gap-x-2 gap-y-1 text-sm/6 text-gray-300">
					Office
					<select aria-label="Office filter" bind:value={filterValue} class="min-w-0 max-w-full rounded-md bg-gray-800 px-2 py-1 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
						<option value="all">All offices</option>
						{#each offices as office (office.id)}<option value={office.id}>{office.name} · {office.host}</option>{/each}
						<option value="unplaced">Unplaced</option>
					</select>
				</label>
				<div class="ml-auto flex max-w-full flex-wrap items-center justify-end gap-2 [&>button]:h-auto [&>button]:min-h-8 [&>button]:max-w-full [&>button]:whitespace-normal">
					<Button size="sm" onclick={() => newTeam()}>{@render plus()} New team</Button>
					<Button variant="outline" size="sm" onclick={() => ((addPersonTeam = undefined), (addPersonOpen = true))}>{@render plus()} Add person</Button>
					<Button variant="outline" size="sm" onclick={() => (addAgentOpen = true)}>{@render plus()} Add agent</Button>
					{@render viewMenu()}
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
				<Callout data-testid="unassigned-note" class="text-sm/6">
					{unassigned.length} {unassigned.length === 1 ? 'project has' : 'projects have'} no team yet.
				</Callout>
			{/if}

			{#if tree.length === 0}
				<div class="rounded-xl bg-white/5 px-6 py-10 text-center outline-1 -outline-offset-1 outline-white/10">
					<p class="text-sm/6 font-semibold text-white">No teams yet</p>
					<p class="mx-auto mt-1 max-w-sm text-sm/6 text-gray-400">Teams group your agents and people. A team can be part of another team, so a whole division is just a team with teams under it.</p>
					<div class="mt-4 flex flex-wrap justify-center gap-3">
						<Button class="h-auto min-h-9 max-w-full whitespace-normal" onclick={() => newTeam()}>Create your first team</Button>
					</div>
				</div>
			{:else if asList}
				<TeamsList {snapshot} {tree} {filter} {highlightLabel} {knownOffices} actions={teamMenu} />
			{:else}
				<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (Keyboard users need to enter this scrollable chart frame.) -->
				<div role="region" aria-label="Teams chart" tabindex="0" onkeydown={onChartFrameKey} class="max-w-full overflow-x-auto rounded-lg p-1 pb-4 outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
					<TeamsChart {snapshot} {tree} {collapsed} {filter} {selected} {fit} {highlightLabel} {knownOffices} {justAdded} actions={teamMenu} onremoveproject={takeProjectOff} onselect={select} />
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
								highlighted={highlightedByOffice(m, filter, knownOffices) === true}
								{highlightLabel}
								dimmed={highlightedByOffice(m, filter, knownOffices) === false}
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
						{#each unassigned as project (project.id)}
							<li class="flex max-w-full flex-wrap items-center gap-x-3 gap-y-2 wrap-anywhere">
								{project.name}
								<Button variant="outline" size="xs" class="h-auto min-h-6 max-w-full whitespace-normal" aria-label="Assign a team to {project.name}" onclick={() => ((assigning = project), (projectTeam = undefined), (projectTeamOpen = true))}>Assign team…</Button>
							</li>
						{/each}
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
			<div class="flex max-w-full flex-wrap items-center justify-end gap-2 [&>button]:h-auto [&>button]:min-h-8 [&>button]:max-w-full [&>button]:whitespace-normal">
				<Button size="sm" onclick={() => ((addPersonTeam = undefined), (addPersonOpen = true))}>{@render plus()} Add person</Button>
			</div>
			{#if tree.length === 0}
				<div class="rounded-xl bg-white/5 px-6 py-10 text-center outline-1 -outline-offset-1 outline-white/10">
					<p class="text-sm/6 font-semibold text-white">No teams yet</p>
				</div>
			{:else}
				<div class="flex max-w-full flex-wrap items-center justify-end gap-2 [&>button]:h-auto [&>button]:min-h-8 [&>button]:max-w-full [&>button]:whitespace-normal">
					{@render viewMenu()}
				</div>
				<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (Keyboard users need to enter this scrollable chart frame.) -->
				<div role="region" aria-label="People chart" tabindex="0" onkeydown={onChartFrameKey} class="max-w-full overflow-x-auto rounded-lg p-1 pb-4 outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
					<PeopleChart {snapshot} {tree} {collapsed} {selected} {fit} onselect={select} onfitblockedchange={onPeopleFitBlocked} />
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
				<div class="flex max-w-full flex-wrap items-center justify-end gap-2 [&>button]:h-auto [&>button]:min-h-8 [&>button]:max-w-full [&>button]:whitespace-normal">
					{@render viewMenu()}
				</div>
				<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (Keyboard users need to enter this scrollable chart frame.) -->
				<div role="region" aria-label="Reporting chart" tabindex="0" onkeydown={onChartFrameKey} class="max-w-full overflow-x-auto rounded-lg p-1 pb-4 outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
					<ReportingChart {snapshot} root={reporting} collapsed={collapsedReports} {selected} {fit} onselect={select} />
				</div>
			{/if}
		</div>
	{/if}
	</div>
</div>

{#if former.length > 0}
	<div class="px-8 pb-6">
		<details class="rounded-xl bg-gray-800/50 p-3 outline-1 -outline-offset-1 outline-white/10" data-testid="former-members">
			<summary class="cursor-pointer text-sm/6 font-semibold text-white">Former members <span class="font-normal text-gray-400">{former.length}</span></summary>
			<ul class="mt-2 space-y-1 text-sm/6 text-gray-300">
				{#each former as m (m.id)}
					<li class="flex flex-wrap items-baseline gap-x-3">
						<span class="font-medium text-white [overflow-wrap:anywhere]">{m.name}</span>
						<span class="text-gray-400">{m.agent?.title ?? ''}</span>
						<span class="text-xs/5 text-gray-400">left {m.agent?.offboarded_at ? new Date(m.agent.offboarded_at).toLocaleDateString(undefined, { dateStyle: 'medium' }) : ''} · the name <code class="font-mono">{m.agent?.messaging_name}</code> stays reserved</span>
					</li>
				{/each}
			</ul>
		</details>
	</div>
{/if}

<NewTeamSheet bind:open={newTeamOpen} snapshot={live} {session} ownerTeamId={newTeamUnder} ondone={teamCreated} />
<RenameTeamDialog bind:open={renameOpen} team={renaming && snapshot.teams.find((x) => x.id === renaming?.id)} snapshot={live} {session} ondone={teamRenamed} />
<DeleteTeamDialog bind:open={deleteOpen} team={deleting && snapshot.teams.find((x) => x.id === deleting?.id)} snapshot={live} {session} ondone={teamDeleted} />
<MoveTeamDialog bind:open={moveOpen} team={moving && snapshot.teams.find((x) => x.id === moving?.id)} snapshot={live} {session} ondone={teamMoved} />
<EditMembersDialog bind:open={membersOpen} team={editingMembers && snapshot.teams.find((x) => x.id === editingMembers?.id)} snapshot={live} {session} ondone={membersEdited} />
<ProjectTeamDialog bind:open={projectTeamOpen} team={projectTeam && snapshot.teams.find((x) => x.id === projectTeam?.id)} project={assigning} snapshot={live} {session} ondone={projectGiven} />
<RemoveProjectDialog bind:open={removeProjectOpen} target={removeProject} {session} ondone={projectGiven} />
<AddPersonSheet bind:open={addPersonOpen} snapshot={full} {session} organization={organizationName} presetTeamId={addPersonTeam} ondone={personAdded} />
<EditPersonDialog bind:open={editPersonOpen} person={editingPerson && snapshot.members.find((x) => x.id === editingPerson?.id)} snapshot={full} {session} organization={organizationName} ondone={personEdited} />
<RemovePersonDialog bind:open={removePersonOpen} person={removingPerson && snapshot.members.find((x) => x.id === removingPerson?.id)} snapshot={live} {session} organization={organizationName} ondone={personRemoved} />
<ChangeManagerDialog bind:open={managerOpen} member={managing && snapshot.members.find((x) => x.id === managing?.id)} snapshot={live} {session} ondone={managerChanged} />
<EditMemberTeamsDialog bind:open={memberTeamsOpen} member={memberTeams && snapshot.members.find((x) => x.id === memberTeams?.id)} snapshot={live} {session} ondone={memberTeamsEdited} />
<AddAgentSheet bind:open={addAgentOpen} snapshot={live} fullSnapshot={full} {session} officeId={data.connection.office.id} officeName={data.connection.office.name} organization={organizationName} ondone={agentAdded} />
<PauseAgentDialog bind:open={pauseOpen} agent={pausing && snapshot.members.find((x) => x.id === pausing?.id)} snapshot={live} {session} ondone={agentPaused} />
<OffboardAgentDialog bind:open={offboardOpen} agent={offboarding && snapshot.members.find((x) => x.id === offboarding?.id)} snapshot={live} {session} ondone={agentOffboarded} />
<EditAgentDialog bind:open={editAgentOpen} agent={editingAgent && snapshot.members.find((x) => x.id === editingAgent?.id)} snapshot={full} {session} ondone={agentEdited} />
<HowItRunsDialog bind:open={howRunsOpen} agent={howRuns && snapshot.members.find((x) => x.id === howRuns?.id)} {session} ondone={agentEdited} />
<MoveMemberDialog bind:open={moveMemberOpen} member={movingMember && snapshot.members.find((x) => x.id === movingMember?.id)} snapshot={live} {session} ondone={memberMoved} />
