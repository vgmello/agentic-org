<!--
  @component
  The Teams index (offices-and-teams-ux §7.2, TEAM-1): the recursive team tree as indented divided rows, each
  with its TEAM- reference, optional lead, People and Agents counts (home team only) and projects marked Owner
  or Contributor. Hire requests, Hire an agent and New team, and each row's menu: Move, Rename, Change lead, Add team under, Archive, Delete.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import Avatar from '$lib/components/Avatar.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Button } from '$lib/components/ui/button';
	import { HIRE_REQUESTS_URL, hireUrl } from '$lib/modules/employees';
	import { orgApi } from '$lib/modules/organization/api';
	import { sessionFor } from '$lib/modules/organization/session';
	import { focusDestinationHeading } from '$lib/platform/workspace/focus-destination';
	import type { OrgSnapshot } from '$lib/proto';
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { teamsApi } from '../../api';
	import { focusAfterRemoval, indentTeams } from '../../tree';
	import TeamDialogs from '../team-actions/TeamDialogs.svelte';
	import TeamMenu from '../team-actions/TeamMenu.svelte';

	let { organizationId, snapshot }: { organizationId: string; snapshot: OrgSnapshot } = $props();

	const teams = new Resource((signal) => teamsApi.list(organizationId, signal));
	const active = $derived((teams.data ?? []).filter((t) => t.status === 'active'));
	const archived = $derived((teams.data ?? []).filter((t) => t.status === 'archived'));
	const rows = $derived(indentTeams(active));
	const summary = $derived.by(() => {
		let people = 0;
		let agents = 0;
		let topLevel = 0;
		const seenProjects: Record<string, true> = {};
		for (const row of rows) {
			if (row.depth === 0) topLevel++;
			people += row.team.people;
			agents += row.team.agents;
			for (const p of row.team.projects) seenProjects[p.id] = true;
		}
		return { people, agents, topLevel, projects: Object.keys(seenProjects).length };
	});
	const ownerInfo = $derived.by(() => {
		const owner = snapshot.members.find((m) => m.owner);
		if (!owner) return undefined;
		const reports = snapshot.members.filter(
			(m) => m.agent?.status !== 'offboarded' && m.reports_to_member_id === owner.id
		);
		return { owner, reports };
	});
	const session = $derived(sessionFor(orgApi, organizationId));
	let dialogs = $state<ReturnType<typeof TeamDialogs>>();
	let justAdded = $state<string | undefined>();

	async function reload() {
		await invalidate('app:org');
		await teams.reload();
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
	const actionBtn = 'min-h-9 max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11';
</script>

<div class="teams-masthead">
	<PageHeader
		title="Teams"
		count={active.length || undefined}
		countLabel="teams"
		description="Who works in which team. Each team can sit under another; a team's lead manages everything under it."
	>
		{#snippet actions()}
			<Button href={resolve(HIRE_REQUESTS_URL as '/')} variant="outline" class={actionBtn}
				>Hire requests</Button
			>
			<Button href={resolve(hireUrl() as '/')} variant="outline" class={actionBtn}
				>Hire an agent</Button
			>
			<Button class={actionBtn} onclick={() => dialogs?.openNew()}>
				<MiniIcon name="plus" class="-ml-0.5 size-5" /> New team
			</Button>
		{/snippet}
	</PageHeader>
</div>

<Loaded
	resource={teams}
	label="teams"
	error="Couldn't load the teams"
	isEmpty={(v) => v.length === 0}
	emptyTitle="No teams yet"
	emptyDescription="Every organization starts with its top-level team. If none shows here, HQ hasn't sent the organization's teams yet."
>
	<div class="flex flex-col gap-5 px-4 py-5 min-[640px]:px-6 min-[1024px]:px-8">
		{#if ownerInfo}
			{@const { owner, reports } = ownerInfo}
			<div
				class="exec-card flex flex-wrap items-center justify-between gap-3 px-4 py-3.5 min-[640px]:px-5"
				data-testid="teams-owner-card"
			>
				<div class="flex min-w-0 items-center gap-3">
					<Avatar name={owner.name} size="md" />
					<div class="min-w-0">
						<p class="flex flex-wrap items-baseline gap-x-1.5 text-sm/6">
							<span class="font-semibold text-fg">{owner.name}</span>
							<span class="text-xs/5 text-fg-muted">· Owner</span>
						</p>
						<p class="text-xs/5 text-fg-muted">
							{#if reports.length > 0}
								{plural(reports.length, 'member reports', 'members report')} directly:
								{reports
									.slice(0, 4)
									.map((m) => m.name)
									.join(', ')}{reports.length > 4 ? ` +${reports.length - 4}` : ''}
							{:else}
								Full access to this organization
							{/if}
						</p>
					</div>
				</div>
				<Button
					href={resolve(`/members/${encodeURIComponent(owner.id)}` as '/')}
					variant="outline"
					size="sm"
					class="min-h-8">Profile</Button
				>
			</div>
		{/if}
		{#if active.length > 0}
			<section
				aria-label="Teams summary"
				class="teams-kpi-strip grid grid-cols-1 gap-3 min-[640px]:grid-cols-2 min-[1024px]:grid-cols-4"
			>
				<div class="teams-kpi-tile flex flex-col gap-1">
					<span class="teams-kpi-label">Active teams</span>
					<p class="teams-kpi-num">{active.length}</p>
					<p class="text-xs/5 text-fg-muted tabular-nums">
						{summary.topLevel} top-level{archived.length > 0
							? ` · ${archived.length} archived`
							: ''}
					</p>
				</div>
				<div class="teams-kpi-tile flex flex-col gap-1">
					<span class="teams-kpi-label inline-flex items-center gap-1.5">
						<span class="t-dot dot-ok" aria-hidden="true"></span>People
					</span>
					<p class="teams-kpi-num">{summary.people}</p>
					<p class="text-xs/5 text-fg-muted">Home-team members</p>
				</div>
				<div class="teams-kpi-tile flex flex-col gap-1">
					<span class="teams-kpi-label inline-flex items-center gap-1.5">
						<span class="t-dot dot-accent" aria-hidden="true"></span>Agents
					</span>
					<p class="teams-kpi-num">{summary.agents}</p>
					<p class="text-xs/5 text-fg-muted">Assigned across teams</p>
				</div>
				<div class="teams-kpi-tile flex flex-col gap-1">
					<span class="teams-kpi-label">Projects</span>
					<p class="teams-kpi-num">{summary.projects}</p>
					<p class="text-xs/5 text-fg-muted">Owned or contributed</p>
				</div>
			</section>
		{/if}

		<!-- At 1024 px and wider the facts line up in columns, so teams can be compared down the page. -->
		<div class="exec-card overflow-hidden">
			<div
				aria-hidden="true"
				class="hidden border-b border-line px-6 py-3 text-xs/5 font-semibold tracking-wider text-fg-muted uppercase min-[1024px]:grid {GRID}"
			>
				<span>Team</span><span>Lead</span><span class="text-right">People</span><span
					class="text-right">Agents</span
				><span>Projects</span><span></span>
			</div>
			<ul role="list" class="divide-y divide-line-subtle" aria-label="Teams, as a tree">
				{#each rows as { team, depth, children } (team.id)}
					<li
						class="relative flex flex-wrap items-start gap-x-3 px-4 py-4 transition-colors duration-150 hover:bg-surface-raised/60 motion-reduce:transition-none min-[640px]:px-6 min-[1024px]:grid {GRID} {justAdded ===
						team.id
							? 'bg-primary/10'
							: ''}"
						data-team={team.id}
						style:--depth={Math.min(depth, 6)}
						style:--tree-step={depth > 0 ? 'min(calc(var(--depth) * 1.5rem), 24%)' : '0rem'}
						style:--indent={depth > 0 ? 'min(calc(var(--depth) * 1.5rem + 0.75rem), 28%)' : '0rem'}
					>
						<div class="flex min-w-0 flex-1 basis-48 items-baseline gap-x-3">
							{#if depth > 0}<span
									aria-hidden="true"
									class="flex shrink-0 justify-end self-center"
									style:width="var(--tree-step)"><span class="h-px w-3 bg-line-strong"></span></span
								>{/if}
							<p class="flex min-w-0 flex-wrap items-center gap-x-2 text-sm/6">
								<a
									href={resolve(`/teams/${team.id}` as '/')}
									data-team-link={team.id}
									class="rounded-sm font-semibold text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
									><span class="absolute inset-0" aria-hidden="true"></span>{team.name}<span
										class="sr-only"
										>, level {depth + 1}{children
											? `, ${plural(children, 'team', 'teams')} under it`
											: ''}</span
									></a
								>
								{#if team.ref}<span
										class="rounded bg-surface-raised px-1.5 py-0.5 font-mono text-xs/4 text-fg-muted ring-1 ring-line ring-inset"
										>{team.ref}</span
									>{/if}
							</p>
						</div>
						<div
							class="relative shrink-0 min-[1024px]:col-start-6 min-[1024px]:row-start-1 min-[1024px]:-my-1.5"
						>
							<TeamMenu {team} onaction={(action, t) => dialogs?.open(action, t)} />
						</div>
						<p
							class="-mt-2 flex w-full flex-wrap gap-x-2 pl-(--indent) text-xs/5 text-fg-muted min-[1024px]:contents min-[1024px]:text-sm/6"
						>
							<!-- Use &#32; because Svelte trims raw leading whitespace inside inline spans. -->
							<span class="min-[1024px]:text-fg-secondary [overflow-wrap:anywhere]"
								>{team.lead ? team.lead.name : 'No lead'}<span class="sr-only">&#32;leads it</span
								><span class="min-[1024px]:hidden">&#32;leads</span></span
							>
							<span aria-hidden="true" class="min-[1024px]:hidden">·</span>
							<span class="tabular-nums min-[1024px]:text-right min-[1024px]:text-fg-secondary"
								>{team.people}<span class="min-[1024px]:sr-only"
									>&#32;{team.people === 1 ? 'person' : 'people'}</span
								></span
							>
							<span aria-hidden="true" class="min-[1024px]:hidden">·</span>
							<span class="tabular-nums min-[1024px]:text-right min-[1024px]:text-fg-secondary"
								>{team.agents}<span class="min-[1024px]:sr-only"
									>&#32;{team.agents === 1 ? 'agent' : 'agents'}</span
								></span
							>
						</p>
						{#if team.projects.length > 0}
							<ul
								role="list"
								class="relative mt-2 flex w-full flex-wrap gap-2 pl-(--indent) min-[1024px]:mt-0 min-[1024px]:w-auto min-[1024px]:pl-0"
								aria-label="Projects of {team.name}"
							>
								{#each team.projects as p (p.id)}
									<li class="max-w-full min-w-0">
										<a
											href={resolve(`/projects/${p.id}` as '/')}
											class={[
												'relative inline-flex min-h-6 max-w-full min-w-0 flex-wrap items-center gap-x-1.5 rounded-xl px-2.5 py-0.5 text-xs/5 font-medium whitespace-normal [overflow-wrap:anywhere] hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11',
												p.role === 'owner'
													? 'team-proj-owner'
													: 'bg-surface-raised text-fg-secondary ring-1 ring-line ring-inset'
											]}
											>{p.name}<span
												class="min-w-0 shrink whitespace-normal [overflow-wrap:anywhere] opacity-80"
												>· {p.role === 'owner' ? 'Owner' : 'Contributor'}</span
											></a
										>
									</li>
								{/each}
							</ul>
						{:else}
							<span class="hidden text-sm/6 text-fg-muted min-[1024px]:block">None</span>
						{/if}
					</li>
				{/each}
			</ul>
		</div>

		{#if archived.length > 0}
			<details class="exec-card overflow-hidden px-4 py-3.5 min-[640px]:px-6">
				<summary
					class="cursor-pointer rounded-sm text-sm/6 font-semibold text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
				>
					Archived teams <span class="font-normal text-fg-muted tabular-nums"
						>{archived.length}</span
					>
				</summary>
				<ul role="list" class="mt-3 flex flex-col gap-2">
					{#each archived as team (team.id)}
						<li class="flex flex-wrap items-center gap-x-2 text-sm/6 text-fg-secondary">
							<a
								href={resolve(`/teams/${team.id}` as '/')}
								class="rounded-sm hover:text-fg focus-visible:outline-2 focus-visible:outline-focus"
								>{team.name}</a
							>
							{#if team.ref}<span class="font-mono text-xs/5 text-fg-muted">{team.ref}</span>{/if}
							<StatusPill label="Archived" />
						</li>
					{/each}
				</ul>
			</details>
		{/if}
	</div>
</Loaded>

<TeamDialogs
	bind:this={dialogs}
	{organizationId}
	{snapshot}
	{session}
	{reload}
	oncreated={created}
	onremoved={async (r) => {
		await refocusAfter(r.teamId);
		toast.success(
			r.action === 'archive' ? `Archived ${r.name}. Its ID and history stay.` : `Deleted ${r.name}.`
		);
	}}
/>

<style>
	.teams-masthead :global(h2) {
		font-size: 1.25rem;
		line-height: 1.75rem;
		letter-spacing: -0.02em;
	}
	:global(html[data-ui-theme^='report']) .teams-masthead :global(header) {
		border-bottom: 1.5px solid var(--report-masthead-rule);
	}
	.teams-kpi-tile {
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: 1rem 1.125rem;
		min-width: 0;
	}
	.teams-kpi-label {
		font-size: 0.7rem;
		font-weight: 650;
		letter-spacing: 0.09em;
		text-transform: uppercase;
		color: var(--exec-muted);
	}
	.teams-kpi-num {
		font-size: 1.5rem;
		line-height: 1.15;
		font-weight: 700;
		letter-spacing: -0.03em;
		color: var(--exec-ink);
		font-variant-numeric: tabular-nums;
	}
	.t-dot {
		width: 0.4375rem;
		height: 0.4375rem;
		border-radius: 999px;
		flex: none;
	}
	.dot-ok {
		background: var(--tone-ok);
	}
	.dot-accent {
		background: var(--tone-accent);
	}
	.team-proj-owner {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
	:global(html[data-ui-theme^='report']) .teams-kpi-strip {
		gap: 0;
		border-bottom: 1px solid var(--report-rule);
		padding-bottom: 0.75rem;
	}
	:global(html[data-ui-theme^='report']) .teams-kpi-tile {
		padding: 0.625rem 0;
	}
	:global(html[data-ui-theme^='report']) .teams-kpi-tile + .teams-kpi-tile {
		border-top: 1px solid var(--report-rule-subtle);
	}
	@media (min-width: 1024px) {
		:global(html[data-ui-theme^='report']) .teams-kpi-tile {
			padding: 0.5rem 1.25rem;
			border-top: 0;
			border-left: 1px solid var(--report-rule-subtle);
		}
		:global(html[data-ui-theme^='report']) .teams-kpi-tile:first-child {
			border-left: 0;
			padding-left: 0;
		}
		:global(html[data-ui-theme^='report']) .teams-kpi-tile + .teams-kpi-tile {
			border-top: 0;
		}
	}
</style>
