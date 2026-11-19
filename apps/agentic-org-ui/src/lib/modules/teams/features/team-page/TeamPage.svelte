<!--
  @component
  A team's page `/teams/:teamId` (offices-and-teams-ux §7.2): back to Teams; the name and TEAM- reference, where it
  sits, its lead, real people and agent counts and its purpose; Hire an agent (the Hire flow with this team preset) and Add a person, and the team's menu (Move, Rename,
  Change lead, Add team under, Archive, Delete). Above the tabs at most six members and All N members. Tabs are
  links (Work, Projects, Activity, Conversations, Processes); changing tab focuses the tab's heading.
  States: loading, unavailable (the route's placeholder), error with Retry, not found, archived.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { afterNavigate, goto, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import Callout from '$lib/components/Callout.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import TabsNav from '$lib/components/TabsNav.svelte';
	import { Button } from '$lib/components/ui/button';
	import { hireUrl } from '$lib/modules/employees';
	import { Resource } from '$lib/components/resource.svelte';
	import { pageTitle } from '$lib/nav';
	import { orgApi } from '$lib/org/api';
	import { sessionFor } from '$lib/org/session';
	import { focusDestinationHeading } from '$lib/platform/shell/focus-destination';
	import type { Connection, OrgSnapshot } from '$lib/proto';
	import { teamsApi } from '../../api';
	import { TEAM_TABS, agentsText, lookUpTeam, peopleText, tabFor, teamHref } from '../../team-page';
	import TeamDialogs from '../team-actions/TeamDialogs.svelte';
	import TeamMenu from '../team-actions/TeamMenu.svelte';
	import { setTeamPageContext } from './context';
	import AddPersonToTeam from './AddPersonToTeam.svelte';
	import TeamMembers from './TeamMembers.svelte';

	let {
		connection,
		snapshot,
		teamId,
		content
	}: {
		connection: Connection;
		snapshot: OrgSnapshot;
		teamId: string;
		/** The current tab; it reads the team from the page's context. */
		content: Snippet;
	} = $props();

	const organizationId = $derived(connection.organization.id);
	const session = $derived(sessionFor(orgApi, organizationId));
	const team = new Resource(() => lookUpTeam(() => teamsApi.team(organizationId, teamId)));
	const tab = $derived(tabFor(page.url.pathname, teamId));
	const tabs = $derived(TEAM_TABS.map((t) => ({ label: t.label, href: teamHref(teamId, t.id) })));
	const detail = $derived(team.data?.found ? team.data.detail : undefined);
	const title = $derived(
		team.state === 'ready'
			? detail
				? pageTitle(tab.id === 'work' ? undefined : tab.label, detail.team.name, 'Teams')
				: pageTitle('Team not found', 'Teams')
			: pageTitle('Team', 'Teams')
	);

	setTeamPageContext({
		get organizationId() {
			return organizationId;
		},
		get teamId() {
			return teamId;
		},
		get teamName() {
			return detail?.team.name ?? '';
		}
	});

	let dialogs = $state<ReturnType<typeof TeamDialogs>>();

	// Changing tab moves focus to the tab's heading, so what was shown is announced (TEAM-10).
	afterNavigate((move) => {
		if (move.type === 'enter' || !move.from || !move.to) return;
		const before = tabFor(move.from.url.pathname, teamId);
		const base = teamHref(teamId);
		const sameTeam = (p: string) => p === base || p.startsWith(`${base}/`);
		if (!sameTeam(move.from.url.pathname) || !sameTeam(move.to.url.pathname)) return;
		if (before.id === tabFor(move.to.url.pathname, teamId).id) return;
		// After the shell's own after-navigation focus (a heading, once a tick has passed), so this one wins.
		setTimeout(() => document.getElementById('team-tab-heading')?.focus(), 0);
	});

	function focusTeamHeading() {
		const heading = document.getElementById('team-heading');
		if (!heading) return void focusDestinationHeading();
		heading.tabIndex = -1;
		heading.focus();
	}

	async function reload() {
		await invalidate('app:org');
		await team.reload();
	}
</script>

<svelte:head><title>{title}</title></svelte:head>

{#if team.state === 'error' || team.state === 'unavailable'}
	<!-- The one named back link stays while the team itself can't be shown. -->
	<div class="px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8">
		<a
			href={resolve('/teams')}
			class="inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
			>Teams</a
		>
	</div>
{/if}
<Loaded resource={team} label="the team" error="Couldn't load this team" place="Team" area="Teams">
	{#snippet children(lookup)}
		{#if !lookup.found}
			<PageHeader
				title="Team not found"
				back={{ href: '/teams', label: 'Teams' }}
				description={lookup.message}
			/>
		{:else}
			{@const d = lookup.detail}
			{@const archived = d.team.status === 'archived'}
			<PageHeader id="team-heading" title={d.team.name} back={{ href: '/teams', label: 'Teams' }}>
				{#snippet description()}
					<p class="flex flex-wrap items-center gap-x-2 gap-y-1">
						{#if d.team.ref}<span class="font-mono text-xs/5 text-gray-400">{d.team.ref}</span><span
								aria-hidden="true">·</span
							>{/if}
						{#if d.team.lead}
							<span
								>Led by <a
									href={resolve(`/members/${encodeURIComponent(d.team.lead.id)}` as '/')}
									class="inline-flex min-h-6 items-center rounded-sm text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 max-[639px]:min-h-11 [overflow-wrap:anywhere]"
									>{d.team.lead.name}</a
								></span
							>
						{:else}
							<span>No lead</span>
						{/if}
						<span aria-hidden="true">·</span>
						<span class="tabular-nums">{peopleText(d.team)}</span>
						<span aria-hidden="true">·</span>
						<span class="tabular-nums">{agentsText(d.team)}</span>
						{#if d.path.length > 0}
							{@const parent = d.path[d.path.length - 1]}
							<span aria-hidden="true">·</span>
							<span
								>Part of <a
									href={resolve(teamHref(parent.id) as '/')}
									class="inline-flex min-h-6 items-center rounded-sm text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 max-[639px]:min-h-11 [overflow-wrap:anywhere]"
									>{parent.name}</a
								></span
							>
						{/if}
						{#if archived}<StatusPill label="Archived" class="ml-1" />{/if}
					</p>
					{#if d.purpose}<p class="mt-2 text-gray-300">{d.purpose}</p>{/if}
				{/snippet}
				{#snippet actions()}
					{#if !archived}
						<Button
							href={resolve(hireUrl({ teamId: d.team.id }) as '/')}
							variant="outline"
							class="max-[639px]:h-11">Hire an agent</Button
						>
						<AddPersonToTeam
							{connection}
							{snapshot}
							{session}
							teamId={d.team.id}
							onadded={reload}
						/>
						<TeamMenu team={d.team} onaction={(action, t) => dialogs?.open(action, t)} />
					{/if}
				{/snippet}
			</PageHeader>
			{#if archived}
				<div class="px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8">
					<Callout>
						<p class="text-sm/6">
							{d.team.name} is archived. Its ID, reference and history stay; it takes no new work.
						</p>
					</Callout>
				</div>
			{/if}
			<TeamMembers detail={d} />
			<TabsNav label="{d.team.name} views" {tabs} />
			<section aria-labelledby="team-tab-heading">
				<h3 id="team-tab-heading" tabindex="-1" class="sr-only">
					{tab.label}
				</h3>
				{@render content()}
			</section>
		{/if}
	{/snippet}
</Loaded>

<TeamDialogs
	bind:this={dialogs}
	{organizationId}
	{snapshot}
	{session}
	{reload}
	oncreated={async (r) => {
		await reload();
		if (r.problems.length === 0) toast.success(`Created ${r.name}.`);
		else
			toast.warning(`Created ${r.name}, but not everything went through.`, {
				description: r.problems.join(' ')
			});
	}}
	onremoved={async (r) => {
		if (r.action === 'delete') {
			await invalidate('app:org');
			await goto(resolve('/teams'));
			await focusDestinationHeading();
			toast.success(`Deleted ${r.name}.`);
		} else {
			await reload();
			focusTeamHeading();
			toast.success(`Archived ${r.name}. Its ID and history stay.`);
		}
	}}
/>
