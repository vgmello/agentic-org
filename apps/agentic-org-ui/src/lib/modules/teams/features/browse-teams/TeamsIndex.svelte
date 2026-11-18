<!--
  @component
  The Teams index (offices-and-teams-ux §7.2, TEAM-1): the recursive team tree as indented divided rows, each
  with its TEAM- reference, optional lead, People and Agents counts (home team only) and projects marked Owner
  or Contributor. New team, and each row's menu: Move, Rename, Change lead, Add team under, Archive, Delete.
-->
<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import NewTeamSheet from '$lib/components/NewTeamSheet.svelte';
	import MoveTeamDialog from '$lib/components/MoveTeamDialog.svelte';
	import RenameTeamDialog from '$lib/components/RenameTeamDialog.svelte';
	import DeleteTeamDialog from '$lib/components/DeleteTeamDialog.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Button } from '$lib/components/ui/button';
	import { toAppError } from '$lib/office/api';
	import { orgApi } from '$lib/org/api';
	import { sessionFor } from '$lib/org/session';
	import { focusDestinationHeading } from '$lib/platform/shell/focus-destination';
	import type { OrgSnapshot } from '$lib/proto';
	import Loaded from '$lib/modules/members/components/Loaded.svelte';
	import { membersApi } from '$lib/modules/members/api';
	import { resource } from '$lib/modules/members/resource.svelte';
	import { teamsApi } from '../../api';
	import { focusAfterRemoval, indentTeams } from '../../tree';
	import type { LeadCandidate, TeamRow } from '../../types';
	import ArchiveTeamDialog from '../archive-team/ArchiveTeamDialog.svelte';
	import ChangeLeadDialog from '../change-lead/ChangeLeadDialog.svelte';
	import TeamRowMenu, { type TeamRowAction } from './TeamRowMenu.svelte';

	let { organizationId, snapshot }: { organizationId: string; snapshot: OrgSnapshot } = $props();

	const teams = resource(() => teamsApi.list(organizationId));
	onMount(() => void teams.load());
	const active = $derived((teams.value ?? []).filter((t) => t.status === 'active'));
	const archived = $derived((teams.value ?? []).filter((t) => t.status === 'archived'));
	const rows = $derived(indentTeams(active));
	const session = $derived(sessionFor(orgApi, organizationId));
	const orgTeam = (row: TeamRow | undefined) => row && snapshot.teams.find((t) => t.id === row.id);

	let newOpen = $state(false);
	let newUnder = $state<string | undefined>();
	let target = $state<TeamRow | undefined>();
	let moveOpen = $state(false);
	let renameOpen = $state(false);
	let deleteOpen = $state(false);
	let archiveOpen = $state(false);
	let leadOpen = $state(false);
	let candidates = $state<LeadCandidate[] | undefined>();
	let justAdded = $state<string | undefined>();

	async function loadCandidates(teamId: string) {
		candidates = undefined;
		try {
			const everyone = await membersApi.directory(organizationId);
			candidates = everyone
				.filter((m) => m.homeTeam?.id === teamId && m.status !== 'inactive')
				.map((m) => ({ id: m.id, name: m.name, kind: m.kind }));
		} catch (e) {
			candidates = [];
			toast.error(`Couldn't load who can lead: ${toAppError(e).message}`);
		}
	}

	function act(action: TeamRowAction, team: TeamRow) {
		target = team;
		if (action === 'move') moveOpen = true;
		else if (action === 'rename') renameOpen = true;
		else if (action === 'delete') deleteOpen = true;
		else if (action === 'archive') archiveOpen = true;
		else if (action === 'lead') {
			leadOpen = true;
			void loadCandidates(team.id);
		} else {
			newUnder = team.id;
			newOpen = true;
		}
	}

	async function reload() {
		await invalidate('app:org');
		await teams.load();
	}

	/** After a row goes away: the next surviving row, else the previous, else the heading (§7.2). */
	async function refocusAfter(removedId: string) {
		const order = rows.map((r) => r.team.id);
		await reload();
		await tick();
		const next = focusAfterRemoval(order, removedId, new Set(rows.map((r) => r.team.id)));
		const link =
			next && document.querySelector<HTMLElement>(`[data-team-link="${CSS.escape(next)}"]`);
		if (link) link.focus();
		else await focusDestinationHeading();
	}

	async function created(r: { teamId: string; name: string; problems: string[] }) {
		await reload();
		justAdded = r.teamId;
		setTimeout(() => justAdded === r.teamId && (justAdded = undefined), 2000);
		await tick();
		document.querySelector<HTMLElement>(`[data-team-link="${CSS.escape(r.teamId)}"]`)?.focus();
		if (r.problems.length === 0) toast.success(`Created ${r.name}.`);
		else
			toast.warning(`Created ${r.name}, but not everything went through.`, {
				description: r.problems.join(' ')
			});
	}

	const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
	const GRID =
		'min-[1024px]:grid-cols-[minmax(0,1fr)_11rem_4.5rem_4.5rem_minmax(0,16rem)_2.25rem] min-[1024px]:items-center min-[1024px]:gap-x-4';
</script>

<PageHeader
	title="Teams"
	count={active.length || undefined}
	countLabel="teams"
	description="Who works in which team. Each team can sit under another; a team's lead manages everything under it."
>
	{#snippet actions()}
		<Button variant="outline" href={resolve('/teams/hires')} class="max-[639px]:h-11"
			>Hire requests</Button
		>
		<Button class="max-[639px]:h-11" onclick={() => ((newUnder = undefined), (newOpen = true))}>
			<MiniIcon name="plus" class="-ml-0.5 size-5" /> New team
		</Button>
	{/snippet}
</PageHeader>

<Loaded
	res={teams}
	label="teams"
	errorTitle="Couldn't load the teams"
	isEmpty={(v) => v.length === 0}
	emptyTitle="No teams yet"
	emptyDescription="Every organization starts with its top-level team. If none shows here, HQ hasn't sent the organization's teams yet."
>
	<!-- At 1024 px and wider the facts line up in columns, so teams can be compared down the page. -->
	<div
		aria-hidden="true"
		class="hidden border-b border-white/10 px-8 py-3 text-xs/5 font-semibold text-gray-400 min-[1024px]:grid {GRID}"
	>
		<span>Team</span><span>Lead</span><span class="text-right">People</span><span class="text-right"
			>Agents</span
		><span>Projects</span><span></span>
	</div>
	<ul role="list" class="divide-y divide-white/5" aria-label="Teams, as a tree">
		{#each rows as { team, depth, children } (team.id)}
			<li
				class="relative flex flex-wrap items-start gap-x-3 px-4 py-4 transition-colors duration-150 min-[640px]:px-6 min-[1024px]:grid min-[1024px]:px-8 {GRID} {justAdded ===
				team.id
					? 'bg-indigo-500/10'
					: ''}"
				data-team={team.id}
				style:--indent={depth > 0 ? `${Math.min(depth, 6) * 1.5 + 0.75}rem` : '0rem'}
			>
				<div class="flex min-w-0 flex-1 basis-48 items-baseline gap-x-3">
					{#if depth > 0}<span
							aria-hidden="true"
							class="flex shrink-0 justify-end self-center"
							style:width="{Math.min(depth, 6) * 1.5}rem"
							><span class="h-px w-3 bg-white/20"></span></span
						>{/if}
					<p class="flex min-w-0 flex-wrap items-baseline gap-x-2 text-sm/6">
						<a
							href={resolve(`/teams/${team.id}` as '/')}
							data-team-link={team.id}
							class="rounded-sm font-semibold text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 [overflow-wrap:anywhere]"
							><span class="absolute inset-0" aria-hidden="true"></span>{team.name}<span
								class="sr-only"
								>, level {depth + 1}{children
									? `, ${plural(children, 'team', 'teams')} under it`
									: ''}</span
							></a
						>
						{#if team.ref}<span class="font-mono text-xs/5 text-gray-400">{team.ref}</span>{/if}
					</p>
				</div>
				<div
					class="relative shrink-0 min-[1024px]:col-start-6 min-[1024px]:row-start-1 min-[1024px]:-my-1.5"
				>
					<TeamRowMenu {team} onaction={act} />
				</div>
				<p
					class="-mt-2 flex w-full flex-wrap gap-x-2 pl-(--indent) text-xs/5 text-gray-400 min-[1024px]:contents min-[1024px]:text-sm/6"
				>
					<!-- The spaces must be mustaches: Svelte trims an element's leading whitespace. -->
					<!-- eslint-disable svelte/no-useless-mustaches -->
					<span class="min-[1024px]:text-gray-300 [overflow-wrap:anywhere]"
						>{team.lead ? team.lead.name : 'No lead'}<span class="sr-only">{' '}leads it</span><span
							class="min-[1024px]:hidden">{' '}leads</span
						></span
					>
					<span aria-hidden="true" class="min-[1024px]:hidden">·</span>
					<span class="tabular-nums min-[1024px]:text-right min-[1024px]:text-gray-300"
						>{team.people}<span class="min-[1024px]:sr-only"
							>{' '}{team.people === 1 ? 'person' : 'people'}</span
						></span
					>
					<span aria-hidden="true" class="min-[1024px]:hidden">·</span>
					<span class="tabular-nums min-[1024px]:text-right min-[1024px]:text-gray-300"
						>{team.agents}<span class="min-[1024px]:sr-only"
							>{' '}{team.agents === 1 ? 'agent' : 'agents'}</span
						></span
					>
					<!-- eslint-enable svelte/no-useless-mustaches -->
				</p>
				{#if team.projects.length > 0}
					<ul
						role="list"
						class="relative mt-2 flex w-full flex-wrap gap-2 pl-(--indent) min-[1024px]:mt-0 min-[1024px]:pl-0 min-[1024px]:w-auto"
						aria-label="Projects of {team.name}"
					>
						{#each team.projects as p (p.id)}
							<li>
								<a
									href={resolve(`/projects/${p.id}` as '/')}
									class="relative inline-flex min-h-6 items-center gap-x-1.5 rounded-md bg-white/5 px-2 text-xs/5 text-gray-300 ring-1 ring-white/10 ring-inset hover:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 max-[639px]:min-h-11"
									>{p.name}<span class="text-gray-400"
										>· {p.role === 'owner' ? 'Owner' : 'Contributor'}</span
									></a
								>
							</li>
						{/each}
					</ul>
				{:else}
					<span class="hidden text-sm/6 text-gray-400 min-[1024px]:block">None</span>
				{/if}
			</li>
		{/each}
	</ul>
	{#if archived.length > 0}
		<details class="border-t border-white/5 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
			<summary
				class="cursor-pointer rounded-sm text-sm/6 font-semibold text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
			>
				Archived teams <span class="font-normal text-gray-400">{archived.length}</span>
			</summary>
			<ul role="list" class="mt-2 space-y-2">
				{#each archived as team (team.id)}
					<li class="flex flex-wrap items-center gap-x-2 text-sm/6 text-gray-300">
						<a
							href={resolve(`/teams/${team.id}` as '/')}
							class="rounded-sm hover:text-white focus-visible:outline-2 focus-visible:outline-indigo-400"
							>{team.name}</a
						>
						{#if team.ref}<span class="font-mono text-xs/5 text-gray-400">{team.ref}</span>{/if}
						<StatusPill label="Archived" />
					</li>
				{/each}
			</ul>
		</details>
	{/if}
</Loaded>

<NewTeamSheet bind:open={newOpen} {snapshot} {session} ownerTeamId={newUnder} ondone={created} />
<MoveTeamDialog
	bind:open={moveOpen}
	team={orgTeam(target)}
	{snapshot}
	{session}
	ondone={async (r) => {
		await reload();
		if (!('stale' in r)) toast.success(`Moved ${r.name} to ${r.to}.`);
	}}
/>
<RenameTeamDialog
	bind:open={renameOpen}
	team={orgTeam(target)}
	{snapshot}
	{session}
	ondone={async (r) => {
		await reload();
		if (r.name) toast.success(`Renamed to ${r.name}.`);
	}}
/>
<DeleteTeamDialog
	bind:open={deleteOpen}
	team={orgTeam(target)}
	{snapshot}
	{session}
	ondone={async (r) => {
		if ('stale' in r) return reload();
		await refocusAfter(target!.id);
		toast.success(`Deleted ${r.name}.`);
	}}
/>
<ArchiveTeamDialog
	bind:open={archiveOpen}
	team={target}
	{organizationId}
	ondone={async (r) => {
		await refocusAfter(target!.id);
		toast.success(`Archived ${r.name}. Its ID and history stay.`);
	}}
/>
<ChangeLeadDialog
	bind:open={leadOpen}
	team={target}
	{candidates}
	{session}
	ondone={async (r) => {
		await reload();
		toast.success(
			r.lead
				? `${r.lead} now leads ${r.team} and manages its subtree.`
				: `${r.team} has no lead now.`
		);
	}}
/>
