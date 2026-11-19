<!--
  @component
  Projects `/projects` (work-items-ux §3, PRJ-5, costs-and-insights-ux §1): every project A–Z, archived ones last, as
  divided rows: name and P- reference (opens the project), the accountable team apart from the contributors, open and
  blocked tasks and done of total, open bugs by severity with Not set, and the Investment column (total to date, this
  month beside it) for viewers allowed amounts; others see no amounts. New project is the one primary action, for
  viewers who may create; `?new` opens it, `&team=<id>` with that team accountable (a team's entry point, §7.9).
  States: loading, unavailable, error with Retry, no projects (the empty state holds New project), the last copy while
  HQ can't be reached (New project disabled with the reason).
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import { goto, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { focusHeading } from '$lib/components/focus';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { projectsApi } from '../../api';
	import { investmentResource, investmentWords } from '../../investment';
	import {
		CHANGES_PAUSED,
		bugParts,
		bugsText,
		openBugs,
		doneText,
		openText,
		projectHref,
		projectRef,
		sortProjects
	} from '../../present';
	import type { ProjectRow } from '../../types';
	import { fact, rowTitleLink } from '../../components/classes';
	import SeverityMark from '../../components/SeverityMark.svelte';
	import StaleNotice from '../../components/StaleNotice.svelte';
	import TeamLink from '../../components/TeamLink.svelte';
	import { focusProjectOnArrival } from '../frame/arrival';
	import NewProjectSheet from '../new-project/NewProjectSheet.svelte';

	let { organizationId }: { organizationId: string } = $props();

	const resource = new Resource(() => projectsApi.list(organizationId));
	const investment = investmentResource(() => organizationId);
	const overview = $derived(resource.data);
	const rows = $derived(overview ? sortProjects(overview.projects) : []);
	const amounts = $derived(investment.data?.allowed ? investment.data.value : undefined);
	const canCreate = $derived(!!overview?.viewer.canCreate);
	const paused = $derived(!!overview?.stale);

	let sheet = $state<ReturnType<typeof NewProjectSheet>>();
	let opener: HTMLElement | null = null;
	let leaving = false;

	function newProject(event?: MouseEvent, teamId?: string) {
		opener = (event?.currentTarget as HTMLElement | undefined) ?? null;
		sheet?.start(teamId);
	}

	// `?new` (and `&team=`) opens New project once the list says the viewer may create — from the New menu, Home or a
	// team's page. The address then drops `new` (replacing the entry), so Back and a reload don't reopen it. An effect,
	// because it acts on the address (outside state) once the read and the sheet exist; it derives nothing.
	$effect(() => {
		if (!page.url.searchParams.has('new') || !overview || !canCreate || paused || !sheet) return;
		const team = page.url.searchParams.get('team') ?? undefined;
		const url = new URL(page.url);
		url.searchParams.delete('new');
		url.searchParams.delete('team');
		untrack(() => {
			void goto(resolve(`${url.pathname}${url.search}` as '/'), {
				replaceState: true,
				keepFocus: true,
				noScroll: true
			}).then(() => newProject(undefined, team));
		});
	});

	async function created(c: { projectId: string; number: number; name: string }) {
		leaving = true;
		// The navigation and search list projects from the organization's structure: read it again first.
		await invalidate('app:org');
		focusProjectOnArrival(c.projectId);
		await goto(resolve(projectHref(c.projectId) as '/'));
		toast.success(`Created ${c.name} (P-${c.number}).`);
	}

	function restoreFocus(event: Event) {
		event.preventDefault();
		if (leaving) return;
		if (opener?.isConnected) opener.focus();
		else focusHeading('projects-heading');
	}

	const columns = 'min-[1024px]:grid min-[1024px]:items-start min-[1024px]:gap-x-6';
	const withMoney = 'min-[1024px]:grid-cols-[minmax(0,1fr)_11rem_13rem_10rem]';
	const withoutMoney = 'min-[1024px]:grid-cols-[minmax(0,1fr)_11rem_13rem]';
</script>

{#snippet createButton(idPrefix: string)}
	<div class="flex flex-col items-start gap-y-1 {idPrefix === 'empty' ? 'items-center' : ''}">
		<Button
			class={mobileButton}
			disabled={paused}
			aria-describedby={paused ? `${idPrefix}-paused` : undefined}
			onclick={(e: MouseEvent) => newProject(e)}
			><MiniIcon name="plus" class="-ml-0.5 size-5" />New project</Button
		>
		{#if paused}<p id="{idPrefix}-paused" class="max-w-60 text-xs/5 text-fg-muted">
				{CHANGES_PAUSED}
			</p>{/if}
	</div>
{/snippet}

{#snippet money(r: ProjectRow)}
	{@const m = investmentWords(
		amounts?.byProject.get(r.id),
		amounts?.coverage,
		amounts?.scope === 'organization' || !!amounts?.byProject.has(r.id)
	)}
	<p class="text-sm/6 text-fg-secondary tabular-nums min-[1024px]:text-right">
		<span class="text-fg-muted min-[1024px]:sr-only">Investment:</span>
		{m.toDate}
	</p>
	{#if m.thisMonth}<p class="text-xs/5 text-fg-muted tabular-nums min-[1024px]:text-right">
			{m.thisMonth}
		</p>{/if}
	{#if m.note}<p class="text-xs/5 text-fg-muted min-[1024px]:text-right">
			{m.note}{#if m.detail}<span class="sr-only"
					>: {m.detail.replace(/^Usage incomplete: /, '')}</span
				>{/if}
		</p>{/if}
{/snippet}

<PageHeader
	id="projects-heading"
	title="Projects"
	count={overview?.projects.length}
	countLabel={overview?.projects.length === 1 ? 'project' : 'projects'}
	description="What the organization's teams work on. Each project has one accountable team; other teams can contribute."
>
	{#snippet actions()}
		{#if overview && canCreate && overview.projects.length > 0}
			{@render createButton('header')}
		{/if}
	{/snippet}
</PageHeader>

{#if overview?.stale}
	<StaleNotice
		stale={overview.stale}
		retrying={resource.reloading}
		onretry={() => resource.reload()}
	/>
{/if}

<Loaded {resource} label="projects" error="Couldn't load the projects">
	{#snippet children(data)}
		{#if data.projects.length > 0 && investment.state === 'loading'}
			<!-- Rows wait for the investment read, so its column never appears under the pointer (§6a). -->
			<SkeletonRows label="projects" rows={Math.min(data.projects.length, 6)} />
		{:else if data.projects.length === 0}
			<EmptyState
				title="No projects yet"
				level={3}
				description={canCreate
					? 'A project holds work your teams do together: its tasks, bugs, docs and repositories. Creating one also creates its team, unless you choose an existing one.'
					: 'A project holds work your teams do together. The organization’s owner or a team lead creates them.'}
			>
				{#snippet action()}
					{#if canCreate}{@render createButton('empty')}{/if}
				{/snippet}
			</EmptyState>
		{:else}
			{#if investment.state === 'error'}
				<p
					class="px-4 pt-4 text-xs/5 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8"
					role="status"
				>
					Couldn't load the investment figures: {investment.message}
					<button
						type="button"
						class="ml-1 inline-flex min-h-6 items-center rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
						disabled={investment.reloading}
						onclick={() => investment.reload()}
						>{investment.reloading ? 'Retrying…' : 'Retry'}</button
					>
				</p>
			{/if}
			<div
				class="hidden border-b border-line px-4 py-3 text-xs/5 font-semibold text-fg-muted min-[640px]:px-6 min-[1024px]:px-8 {columns} {amounts
					? withMoney
					: withoutMoney}"
				aria-hidden="true"
			>
				<span>Project</span>
				<span>Tasks</span>
				<span>Open bugs</span>
				{#if amounts}<span class="text-right">Investment</span>{/if}
			</div>
			<ul role="list" class="divide-y divide-line-subtle" aria-labelledby="projects-heading">
				{#each rows as r (r.id)}
					<li
						class="relative px-4 py-4 transition-colors duration-150 hover:bg-tint/[0.02] motion-reduce:transition-none min-[640px]:px-6 min-[1024px]:px-8 {columns} {amounts
							? withMoney
							: withoutMoney}"
						data-testid="project-row"
						data-project={r.id}
					>
						<div class="min-w-0">
							<p class="flex flex-wrap items-baseline gap-x-2 text-sm/6">
								<a href={resolve(projectHref(r.id) as '/')} class={rowTitleLink}
									><span class="absolute inset-0" aria-hidden="true"></span>{r.name}</a
								>
								<span class="font-mono text-xs/5 text-fg-muted">{projectRef(r)}</span>
								{#if r.archived}<span
										class="inline-flex shrink-0 items-center rounded-md bg-surface-raised px-2 text-xs/5 font-medium whitespace-nowrap text-fg-secondary ring-1 ring-line ring-inset"
										>Archived</span
									>{/if}
							</p>
							<ul role="list" class="mt-1 space-y-0.5 text-xs/5 text-fg-muted">
								<li class={fact} data-testid="owner-team">
									Accountable team: <TeamLink team={r.ownerTeam} size="xs" />
								</li>
								{#if r.contributors.length}
									<li class={fact} data-testid="contributors">
										Contributors: {#each r.contributors as t, i (t.id)}{#if i > 0},
											{/if}<TeamLink team={t} size="xs" />{/each}
									</li>
								{/if}
							</ul>
						</div>
						<div class="mt-2 min-[1024px]:mt-0">
							<p class="text-sm/6 text-fg-secondary tabular-nums" data-testid="open-tasks">
								{openText(r.tasks)}
							</p>
							<p class="text-xs/5 text-fg-muted tabular-nums">{doneText(r.tasks)}</p>
						</div>
						<div class="mt-1 min-[1024px]:mt-0" data-testid="bugs">
							<p class="text-sm/6 text-fg-secondary tabular-nums">
								<span class="sr-only">{bugsText(r.bugs)}</span><span aria-hidden="true"
									>{openBugs(r.bugs) === 0
										? 'No open bugs'
										: `${openBugs(r.bugs)} open ${openBugs(r.bugs) === 1 ? 'bug' : 'bugs'}`}</span
								>
							</p>
							{#if openBugs(r.bugs) > 0}
								<p class="flex flex-wrap gap-x-2 text-xs/5 text-fg-muted" aria-hidden="true">
									{#each bugParts(r.bugs) as part (part.key)}<SeverityMark
											severity={part.key}
											text={part.text}
										/>{/each}
								</p>
							{/if}
						</div>
						{#if amounts}
							<div class="mt-1 min-[1024px]:mt-0" data-testid="investment">
								{@render money(r)}
							</div>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	{/snippet}
</Loaded>

<NewProjectSheet
	bind:this={sheet}
	{organizationId}
	oncreated={created}
	onCloseAutoFocus={restoreFocus}
/>
