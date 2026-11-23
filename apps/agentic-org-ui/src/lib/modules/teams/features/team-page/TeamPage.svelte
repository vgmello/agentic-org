<!--
  @component
  A team's page `/teams/:teamId` (offices-and-teams-ux §7.2): back to Teams; the name and TEAM- reference, where it
  sits, its lead, real people and agent counts and its purpose; Hire an agent (the Hire flow with this team preset) and Add a person, and the team's menu (Move, Rename,
  Change lead, Add team under, Archive, Delete). Above the tabs at most six members and All N members. Tabs are
  links (Work, Projects, Activity, Conversations, Processes); changing tab focuses the tab's heading.
  States: loading (a skeleton shaped like the page), unavailable (said in place), error with Retry, not found,
  archived; the back link to Teams shows in each.
-->
<script lang="ts">
	import { tick, type Snippet } from 'svelte';
	import { afterNavigate, goto, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import Callout from '$lib/components/Callout.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
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
	import { fact, factsList, factsWrap, linkInLine } from './facts';
	import TeamMembers from './TeamMembers.svelte';
	import TeamPageSkeleton from './TeamPageSkeleton.svelte';
	import { reloadOnChange } from './reload.svelte';

	let {
		connection,
		snapshot,
		teamId,
		loads,
		content
	}: {
		connection: Connection;
		snapshot: OrgSnapshot;
		teamId: string;
		/** The route's load count: every `invalidate('app:org')` moves it, and the page re-reads the team. */
		loads: number;
		/** The current tab; it reads the team from the page's context. */
		content: Snippet;
	} = $props();

	const organizationId = $derived(connection.organization.id);
	const session = $derived(sessionFor(orgApi, organizationId));
	const team = new Resource(() => lookUpTeam(() => teamsApi.team(organizationId, teamId)));
	// Live changes, New › Add a person and this page's own actions apply in place, keeping what's shown until then.
	reloadOnChange(team, () => loads);
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
		},
		get loads() {
			return loads;
		}
	});

	let dialogs = $state<ReturnType<typeof TeamDialogs>>();
	// A link inside the 14/24 facts line: a 24 px target, 44 px below 640 px, without growing the line (SHELL-33).
	const inlineLink = linkInLine.sm;

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

	/** After a change made here: the route reloads and `reloadOnChange` re-reads; read here only if it didn't. */
	async function reload() {
		const before = loads;
		await invalidate('app:org');
		await tick();
		if (loads === before && !team.reloading) await team.reload();
	}
</script>

<svelte:head><title>{title}</title></svelte:head>

<!-- The one named back link, in every state: loading, failed, unavailable, not found and found. -->
<div class="px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8">
	<a
		href={resolve('/teams')}
		class="inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
		>Teams</a
	>
</div>
{#if team.state === 'loading'}
	<TeamPageSkeleton />
{:else}
	<!-- No `area`: an unavailable read says this server doesn't serve the team yet, not that the page isn't built. -->
	<Loaded resource={team} label="the team" error="Couldn't load this team">
		{#snippet children(lookup)}
			{#if !lookup.found}
				<header
					class="border-b border-line-subtle px-4 pt-2 pb-6 min-[640px]:px-6 min-[1024px]:px-8"
				>
					<h2 id="team-heading" class="text-base/7 font-semibold text-fg [overflow-wrap:anywhere]">
						Team not found
					</h2>
					<p class="mt-1 max-w-3xl text-sm/6 text-fg-muted">{lookup.message}</p>
				</header>
			{:else}
				{@const d = lookup.detail}
				{@const archived = d.team.status === 'archived'}
				{@const parent = d.path.at(-1)}
				<header
					class="team-masthead border-b border-line px-4 pt-2 pb-6 min-[640px]:px-6 min-[1024px]:px-8"
				>
					<div class="flex flex-wrap items-start justify-between gap-x-6 gap-y-4">
						<!-- Wide enough that the facts don't wrap one at a time: the actions move below first. -->
						<div class="min-w-0 flex-1 basis-80">
							<h2
								id="team-heading"
								class="flex flex-wrap items-baseline gap-x-2 text-xl/7 font-semibold tracking-tight text-fg [overflow-wrap:anywhere]"
							>
								{d.team.name}
							</h2>
							<div class="mt-1">
								<div class={factsWrap}>
									<ul role="list" class="{factsList} items-baseline text-sm/6 text-fg-muted">
										{#if d.team.ref}<li class="{fact} font-mono text-xs/5">{d.team.ref}</li>{/if}
										<li class={fact}>
											{#if d.team.lead}Led by <a
													href={resolve(`/members/${encodeURIComponent(d.team.lead.id)}` as '/')}
													class={inlineLink}>{d.team.lead.name}</a
												>{:else}No lead{/if}
										</li>
										<li class="{fact} tabular-nums">{peopleText(d.team)}</li>
										<li class="{fact} tabular-nums">{agentsText(d.team)}</li>
										{#if parent}
											<li class={fact}>
												Part of <a href={resolve(teamHref(parent.id) as '/')} class={inlineLink}
													>{parent.name}</a
												>
											</li>
										{/if}
										{#if archived}<li class="{fact} self-center before:hidden">
												<StatusPill label="Archived" class="ml-1" />
											</li>{/if}
									</ul>
								</div>
							</div>
							{#if d.purpose}<p class="mt-2 max-w-3xl text-sm/6 text-fg-secondary">
									{d.purpose}
								</p>{/if}
						</div>
						{#if !archived}
							<div class="flex flex-wrap items-center gap-3">
								<Button
									href={resolve(hireUrl({ teamId: d.team.id }) as '/')}
									variant="outline"
									class="min-h-9 max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
									>Hire an agent</Button
								>
								<AddPersonToTeam {connection} {snapshot} teamId={d.team.id} onadded={reload} />
								<TeamMenu team={d.team} onaction={(action, t) => dialogs?.open(action, t)} />
							</div>
						{/if}
					</div>
				</header>
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
				<!-- Five tabs wrap onto a second line rather than scroll out of sight (narrow widths, large text). -->
				<TabsNav label="{d.team.name} views" {tabs} class="flex-wrap overflow-x-visible" />
				<section aria-labelledby="team-tab-heading">
					<h3 id="team-tab-heading" tabindex="-1" class="sr-only">
						{tab.label}
					</h3>
					{@render content()}
				</section>
			{/if}
		{/snippet}
	</Loaded>
{/if}

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
			// Leave first: re-reading here would show the deleted team as "Team not found" on the way out. The Teams
			// index reads the organization afresh; the shell's lists follow once this page is gone.
			await goto(resolve('/teams'));
			await focusDestinationHeading();
			toast.success(`Deleted ${r.name}.`);
			void invalidate('app:org');
		} else {
			await reload();
			focusTeamHeading();
			toast.success(`Archived ${r.name}. Its ID and history stay.`);
		}
	}}
/>

<style>
	:global(html[data-ui-theme^='report']) .team-masthead {
		border-bottom: 1.5px solid var(--report-masthead-rule);
	}
</style>
