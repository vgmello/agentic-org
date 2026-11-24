<!--
  @component
  A team's page `/teams/:teamId` (Prototype Release 1149b4f1, offices-and-teams-ux §7.2):
  back to Teams; unified .section-header with name, TEAM- reference, lead, real people/agent counts, and parent link;
  shared .section-toolbar with primary tabs (Overview, Members, Work, Projects, Activity, Conversations, Processes),
  optional tab controls, and primary actions (Hire an agent, Add a person, Team menu). Full roster lives in Members.
-->
<script lang="ts">
	import { onDestroy, tick, type Snippet } from 'svelte';
	import { afterNavigate, beforeNavigate, goto, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import { toast } from 'svelte-sonner';
	import Callout from '$lib/components/Callout.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import TabsNav, { type Tab } from '$lib/components/TabsNav.svelte';
	import { Button } from '$lib/components/ui/button';
	import { hireUrl } from '$lib/modules/employees';
	import { Resource } from '$lib/components/resource.svelte';
	import { pageTitle } from '$lib/nav';
	import { orgApi } from '$lib/modules/organization/api';
	import { sessionFor } from '$lib/modules/organization/session';
	import { focusDestinationHeading } from '$lib/platform/shell/focus-destination';
	import type { Connection, OrgSnapshot } from '$lib/proto';
	import { teamsApi } from '../../api';
	import { TEAM_TABS, agentsText, lookUpTeam, peopleText, tabFor, teamHref } from '../../team-page';
	import TeamDialogs from '../team-actions/TeamDialogs.svelte';
	import TeamMenu from '../team-actions/TeamMenu.svelte';
	import { setTeamPageContext } from './context';
	import AddPersonToTeam from './AddPersonToTeam.svelte';
	import { fact, factsList, factsWrap, linkInLine } from './facts';
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
	const work = new Resource(() => teamsApi.work(organizationId, teamId));
	// Live changes, New › Add a person and this page's own actions apply in place, keeping what's shown until then.
	reloadOnChange(team, () => loads);
	reloadOnChange(work, () => loads);
	const tab = $derived(tabFor(page.url.pathname, teamId));
	const detail = $derived(team.data?.found ? team.data.detail : undefined);
	let toolbarControls = $state.raw<Snippet | undefined>(undefined);
	const openWorkCount = $derived(
		work.state === 'ready' && work.data
			? work.data.filter((r) => r.state !== 'done').length
			: undefined
	);

	const tabs = $derived<Tab[]>(
		TEAM_TABS.map((t) => ({
			label: t.label,
			href: teamHref(teamId, t.id),
			count:
				t.id === 'members' ? detail?.members.length : t.id === 'work' ? openWorkCount : undefined
		}))
	);
	const title = $derived(
		team.state === 'ready'
			? detail
				? pageTitle(tab.id === 'overview' ? undefined : tab.label, detail.team.name, 'Teams')
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
		get detail() {
			return detail!;
		},
		get loads() {
			return loads;
		},
		get work() {
			return work;
		},
		setToolbarControls: (controls) => {
			toolbarControls = controls;
		}
	});

	let dialogs = $state<ReturnType<typeof TeamDialogs>>();
	// A link inside the 14/24 facts line: a 24 px target, 44 px below 640 px, without growing the line (SHELL-33).
	const inlineLink = linkInLine.sm;

	// Changing tab moves focus to the tab's heading, so what was shown is announced (TEAM-10),
	// unless a modal dialog (such as Team Work's reading panel for ?item=...) already owns focus.
	let tabFocusTimer: ReturnType<typeof setTimeout> | undefined;
	onDestroy(() => clearTimeout(tabFocusTimer));
	beforeNavigate(() => {
		clearTimeout(tabFocusTimer);
		tabFocusTimer = undefined;
	});
	afterNavigate((move) => {
		clearTimeout(tabFocusTimer);
		tabFocusTimer = undefined;
		if (move.type === 'enter' || !move.from || !move.to) return;
		const before = tabFor(move.from.url.pathname, teamId);
		const base = teamHref(teamId);
		const sameTeam = (p: string) => p === base || p.startsWith(`${base}/`);
		if (!sameTeam(move.from.url.pathname) || !sameTeam(move.to.url.pathname)) return;
		if (before.id === tabFor(move.to.url.pathname, teamId).id) return;
		// After the shell's own after-navigation focus (a heading, once a tick has passed), so this one wins.
		tabFocusTimer = setTimeout(() => {
			if (document.querySelector('[role="dialog"], [role="alertdialog"]')) return;
			document.getElementById('team-tab-heading')?.focus();
		}, 0);
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
<div class="px-4 pt-5 min-[640px]:px-6 min-[1024px]:px-8">
	<a
		href={resolve('/teams')}
		class="inline-flex min-h-6 items-center gap-1 rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
		><ChevronLeft class="size-4 shrink-0" aria-hidden="true" />Teams</a
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
				<div class="section-header team-header">
					<header class="team-masthead ph px-4 pt-2 pb-5 min-[640px]:px-6 min-[1024px]:px-8">
						<div class="min-w-0">
							<h2
								id="team-heading"
								tabindex="-1"
								class="flex flex-wrap items-baseline gap-x-2 text-xl/7 font-semibold tracking-tight text-fg [overflow-wrap:anywhere] focus:outline-none"
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
						</div>
					</header>
					<div class="section-toolbar team-toolbar">
						<div class="primary-tab-row">
							<!-- Tabs wrap onto a second line rather than scroll out of sight (narrow widths, large text). -->
							<TabsNav
								label="{d.team.name} views"
								{tabs}
								class="tabs flex-wrap overflow-x-visible"
							/>
						</div>
						{#if toolbarControls}
							<div class="queue-controls" role="group" aria-label="View controls">
								{@render toolbarControls()}
							</div>
						{/if}
						{#if !archived}
							<div class="section-actions flex flex-wrap items-center gap-2">
								<Button
									href={resolve(hireUrl({ teamId: d.team.id }) as '/')}
									variant="outline"
									size="sm"
									class="min-h-9 max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11"
									>Hire an agent</Button
								>
								<AddPersonToTeam {connection} {snapshot} teamId={d.team.id} onadded={reload} />
								<TeamMenu team={d.team} onaction={(action, t) => dialogs?.open(action, t)} />
							</div>
						{/if}
					</div>
				</div>
				{#if archived}
					<div class="px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8">
						<Callout>
							<p class="text-sm/6">
								{d.team.name} is archived. Its ID, reference and history stay; it takes no new work.
							</p>
						</Callout>
					</div>
				{/if}
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
	.section-header {
		position: relative;
		z-index: 1;
		flex: none;
		container-type: inline-size;
		--team-gutter: 1rem;
	}
	@media (min-width: 640px) {
		.section-header {
			--team-gutter: 1.5rem;
		}
	}
	@media (min-width: 1024px) {
		.section-header {
			--team-gutter: 2rem;
		}
	}
	.section-toolbar {
		position: relative;
		display: flex;
		flex-wrap: wrap;
		align-items: stretch;
		gap: 6px 12px;
		padding: 4px var(--team-gutter) 1px;
		border-block-end: 1px solid var(--line);
	}
	.section-toolbar > :not(.primary-tab-row) {
		align-self: center;
	}
	.primary-tab-row {
		position: relative;
		display: flex;
		align-items: stretch;
		min-inline-size: 0;
		max-inline-size: 100%;
	}
	.primary-tab-row::after {
		content: '';
		position: absolute;
		inset-block-start: 100%;
		inset-inline-start: calc(-1 * var(--team-gutter));
		inline-size: 100cqi;
		border-block-start: 1px solid var(--line);
		pointer-events: none;
	}
	.primary-tab-row > :global(nav) {
		min-width: 0;
		max-width: 100%;
		padding-inline: 0;
		border-bottom: 0;
	}
	.queue-controls {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
		margin-block: 4px;
	}
	.section-actions {
		margin-inline-start: auto;
		margin-block: 4px;
	}
	@container (max-width: 40rem) {
		.primary-tab-row {
			flex-basis: 100%;
		}
	}
	:global(html[data-ui-theme^='report']) .team-toolbar {
		border-block-end: 1.5px solid var(--report-masthead-rule);
	}
</style>
