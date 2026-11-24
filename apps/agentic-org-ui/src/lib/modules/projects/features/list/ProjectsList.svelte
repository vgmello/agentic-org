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
	import { investmentResource, investmentWords, portfolioInvestmentWords } from '../../investment';
	import {
		CHANGES_PAUSED,
		bugParts,
		bugsText,
		countedTasks,
		openBugs,
		doneText,
		openText,
		progressPercent,
		projectBarTone,
		projectHealthLabel,
		projectHref,
		projectRef,
		projectsPortfolioSummary,
		sortProjects,
		type ProjectBarTone
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
	const summary = $derived(projectsPortfolioSummary(rows));
	let showFilter = $state<'all' | 'active' | 'archived'>('all');
	const filteredRows = $derived(
		showFilter === 'active'
			? rows.filter((r) => !r.archived)
			: showFilter === 'archived'
				? rows.filter((r) => r.archived)
				: rows
	);
	const amounts = $derived(investment.data?.allowed ? investment.data.value : undefined);
	const orgTotalInvestment = $derived(portfolioInvestmentWords(amounts));
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

	// Container-driven columns so the Project column never collapses below 12rem when the sidebar narrows main.
	const withMoney =
		'@min-[54rem]:grid @min-[54rem]:items-start @min-[54rem]:gap-x-6 @min-[54rem]:grid-cols-[minmax(12rem,1fr)_12.5rem_12.5rem_9.5rem]';
	const withoutMoney =
		'@min-[44rem]:grid @min-[44rem]:items-start @min-[44rem]:gap-x-6 @min-[44rem]:grid-cols-[minmax(12rem,1fr)_12.5rem_12.5rem]';

	const barFill: Record<ProjectBarTone, string> = {
		ok: 'var(--tone-ok-bar)',
		warn: 'var(--tone-warn-bar)',
		bad: 'var(--tone-bad-bar)',
		accent: 'var(--tone-accent-bar)',
		archived: 'var(--quiet)'
	};

	function healthPillClass(tone: ProjectBarTone): string {
		if (tone === 'bad') return 't-red';
		if (tone === 'warn') return 't-amber';
		if (tone === 'ok') return 't-green';
		if (tone === 'accent') return 'proj-pill-accent';
		return 'bg-surface-raised text-fg-secondary ring-1 ring-line ring-inset';
	}
</script>

{#snippet createButton(idPrefix: string)}
	<div class={['flex flex-col gap-y-1', idPrefix === 'empty' ? 'items-center' : 'items-start']}>
		<Button
			class="min-h-9 {mobileButton} [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
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
	<p class="text-sm/6 font-medium text-fg-secondary tabular-nums @min-[54rem]:text-right">
		<span class="text-fg-muted @min-[54rem]:sr-only">Investment:</span>
		{m.toDate}
	</p>
	{#if m.thisMonth}<p class="text-xs/5 text-fg-muted tabular-nums @min-[54rem]:text-right">
			{m.thisMonth}
		</p>{/if}
	{#if m.note}<p class="text-xs/5 text-fg-muted @min-[54rem]:text-right">
			{m.note}{#if m.detail}<span class="sr-only"
					>: {m.detail.replace(/^Usage incomplete: /, '')}</span
				>{/if}
		</p>{/if}
{/snippet}

<div class="projects-masthead section-header project-header">
	<PageHeader
		id="projects-heading"
		title="Projects"
		count={overview?.projects.length}
		countLabel={overview?.projects.length === 1 ? 'project' : 'projects'}
		description="What the organization's teams work on. Each project has one accountable team; other teams can contribute."
		class="ph"
	/>
	{#if overview && overview.projects.length > 0}
		<div class="section-toolbar project-toolbar project-directory-tools">
			<div class="queue-controls" role="group" aria-label="Filter projects">
				<button
					type="button"
					class="project-filter-btn"
					aria-pressed={showFilter === 'all'}
					onclick={() => (showFilter = 'all')}
				>
					<span>All</span>
					<span class="project-filter-cnt tabular-nums">{rows.length}</span>
				</button>
				<button
					type="button"
					class="project-filter-btn"
					aria-pressed={showFilter === 'active'}
					onclick={() => (showFilter = 'active')}
				>
					<span>Active</span>
					<span class="project-filter-cnt tabular-nums">{summary.active}</span>
				</button>
				{#if summary.archived > 0}
					<button
						type="button"
						class="project-filter-btn"
						aria-pressed={showFilter === 'archived'}
						onclick={() => (showFilter = 'archived')}
					>
						<span>Archived</span>
						<span class="project-filter-cnt tabular-nums">{summary.archived}</span>
					</button>
				{/if}
			</div>
			{#if canCreate}
				<div class="project-toolbar-actions">
					{@render createButton('header')}
				</div>
			{/if}
		</div>
	{/if}
</div>

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
			<div class="flex flex-col gap-5 px-4 py-5 min-[640px]:px-6 min-[1024px]:px-8">
				<section
					aria-label="Projects summary"
					class={[
						'proj-kpi-strip grid grid-cols-1 gap-3 min-[640px]:grid-cols-2',
						orgTotalInvestment ? 'min-[1024px]:grid-cols-4' : 'min-[1024px]:grid-cols-3'
					]}
				>
					<div class="proj-kpi-tile flex flex-col gap-1">
						<div class="flex items-center justify-between gap-2">
							<span class="proj-kpi-label">Active projects</span>
							<span
								class={[
									'inline-flex items-center rounded-full px-2 py-0.5 text-[11px]/4 font-semibold',
									summary.attentionProjects > 0 ? 't-amber' : 't-green'
								]}
							>
								{summary.attentionProjects > 0
									? `${summary.attentionProjects} attention`
									: 'On track'}
							</span>
						</div>
						<p class="proj-kpi-num">{summary.active}</p>
						<p class="text-xs/5 text-fg-muted tabular-nums">
							{summary.archived > 0 ? `${summary.archived} archived` : 'All active'}
						</p>
					</div>

					<div class="proj-kpi-tile flex flex-col gap-1">
						<div class="flex items-center justify-between gap-2">
							<span class="proj-kpi-label">Open tasks</span>
							<span
								class={[
									'inline-flex items-center rounded-full px-2 py-0.5 text-[11px]/4 font-semibold tabular-nums',
									summary.blockedTasks > 0 ? 't-amber' : 't-green'
								]}
							>
								{summary.blockedTasks > 0 ? `${summary.blockedTasks} blocked` : 'Clear'}
							</span>
						</div>
						<p class="proj-kpi-num">{summary.openTasks}</p>
						<p class="text-xs/5 text-fg-muted">Across active projects</p>
					</div>

					<div class="proj-kpi-tile flex flex-col gap-1">
						<div class="flex items-center justify-between gap-2">
							<span class="proj-kpi-label">Open bugs</span>
							<span
								class={[
									'inline-flex items-center rounded-full px-2 py-0.5 text-[11px]/4 font-semibold tabular-nums',
									summary.criticalBugs > 0 ? 't-red' : summary.openBugs > 0 ? 't-amber' : 't-green'
								]}
							>
								{summary.criticalBugs > 0
									? `${summary.criticalBugs} critical`
									: summary.openBugs > 0
										? 'Active'
										: 'None open'}
							</span>
						</div>
						<p class="proj-kpi-num">{summary.openBugs}</p>
						<p class="text-xs/5 text-fg-muted">Across active projects</p>
					</div>

					{#if orgTotalInvestment}
						<div class="proj-kpi-tile flex flex-col gap-1" data-testid="portfolio-investment">
							<span class="proj-kpi-label">Investment</span>
							<p class="proj-kpi-num">{orgTotalInvestment.toDate.amount}</p>
							{#if !orgTotalInvestment.thisMonth && !orgTotalInvestment.toDate.qualification && !orgTotalInvestment.toDate.apiEquivalent}
								<p class="text-xs/5 text-fg-muted">To date</p>
							{/if}
							{#if orgTotalInvestment.toDate.qualification}
								<p class="text-xs/5 text-warning-text [overflow-wrap:anywhere]">
									To date: {orgTotalInvestment.toDate.qualification}
								</p>
							{/if}
							{#if orgTotalInvestment.toDate.apiEquivalent}
								<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
									To date: {orgTotalInvestment.toDate.apiEquivalent}
								</p>
							{/if}
							{#if orgTotalInvestment.thisMonth}
								<p class="text-xs/5 font-medium text-fg-secondary tabular-nums">
									{orgTotalInvestment.thisMonth.amount} this month
								</p>
								{#if orgTotalInvestment.thisMonth.qualification}
									<p class="text-xs/5 text-warning-text [overflow-wrap:anywhere]">
										This month: {orgTotalInvestment.thisMonth.qualification}
									</p>
								{/if}
								{#if orgTotalInvestment.thisMonth.apiEquivalent}
									<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
										This month: {orgTotalInvestment.thisMonth.apiEquivalent}
									</p>
								{/if}
							{/if}
						</div>
					{/if}
				</section>

				{#if investment.state === 'error'}
					<p class="text-xs/5 text-fg-muted" role="status">
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

				<div class="exec-card @container overflow-hidden">
					<div
						class="hidden border-b border-line px-4 py-2.5 text-[11px]/5 font-semibold tracking-wider text-fg-muted uppercase min-[640px]:px-6 {amounts
							? withMoney
							: withoutMoney}"
						aria-hidden="true"
					>
						<span>Project</span>
						<span>Tasks &amp; progress</span>
						<span>Open bugs</span>
						{#if amounts}<span class="text-right">Investment</span>{/if}
					</div>
					<ul role="list" class="divide-y divide-line-subtle" aria-labelledby="projects-heading">
						{#if filteredRows.length === 0}
							<li class="px-4 py-8 text-center text-sm/6 text-fg-muted min-[640px]:px-6">
								No projects match this filter.
							</li>
						{/if}
						{#each filteredRows as r (r.id)}
							{@const total = countedTasks(r.tasks)}
							{@const pct = progressPercent(r.tasks)}
							{@const tone = projectBarTone(r.tasks, r.bugs, r.archived)}
							<li
								class="group relative px-4 py-4 transition-colors duration-150 hover:bg-surface-raised motion-reduce:transition-none min-[640px]:px-6 {amounts
									? withMoney
									: withoutMoney}"
								data-testid="project-row"
								data-project={r.id}
							>
								<div class="min-w-0">
									<p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm/6">
										<a href={resolve(projectHref(r.id) as '/')} class={rowTitleLink}
											><span class="absolute inset-0" aria-hidden="true"></span>{r.name}</a
										>
										<span
											class="rounded bg-surface-raised px-1.5 py-0.5 font-mono text-[11px]/4 text-fg-muted ring-1 ring-line ring-inset"
											>{projectRef(r)}</span
										>
										<span
											class={[
												'inline-flex max-w-full shrink-0 items-center rounded-full px-2 py-0.5 text-[11px]/4 font-semibold whitespace-nowrap',
												healthPillClass(tone)
											]}>{projectHealthLabel(tone)}</span
										>
									</p>
									<ul role="list" class="mt-1.5 flex flex-col gap-0.5 text-xs/5 text-fg-muted">
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
								<div
									class={[
										'mt-2.5 flex min-w-0 flex-col gap-1',
										amounts ? '@min-[54rem]:mt-0' : '@min-[44rem]:mt-0'
									]}
								>
									<p
										class={[
											'text-sm/6 tabular-nums',
											r.tasks.blocked > 0 ? 'font-medium text-warning-text' : 'text-fg-secondary'
										]}
										data-testid="open-tasks"
									>
										{openText(r.tasks)}
									</p>
									<div class="flex items-center gap-2">
										<div
											class="h-1.5 flex-1 overflow-hidden rounded-full bg-bar-track"
											role="progressbar"
											aria-label="{r.name} progress"
											aria-valuemin="0"
											aria-valuemax={total}
											aria-valuenow={r.tasks.done}
											aria-valuetext={doneText(r.tasks)}
										>
											<span
												class="block h-full rounded-full transition-[width] duration-300 motion-reduce:transition-none"
												style:width="{pct}%"
												style:background={barFill[tone]}
											></span>
										</div>
										{#if total > 0}
											<span class="shrink-0 text-[11px]/4 font-semibold text-fg-muted tabular-nums"
												>{pct}%</span
											>
										{/if}
									</div>
									<p class="text-xs/5 text-fg-muted tabular-nums">{doneText(r.tasks)}</p>
								</div>
								<div
									class={['mt-2 min-w-0', amounts ? '@min-[54rem]:mt-0' : '@min-[44rem]:mt-0']}
									data-testid="bugs"
								>
									<p class="text-sm/6 text-fg-secondary tabular-nums">
										<span class="sr-only">{bugsText(r.bugs)}</span><span aria-hidden="true"
											>{openBugs(r.bugs) === 0
												? 'No open bugs'
												: `${openBugs(r.bugs)} open ${openBugs(r.bugs) === 1 ? 'bug' : 'bugs'}`}</span
										>
									</p>
									{#if openBugs(r.bugs) > 0}
										<p
											class="mt-1 flex flex-wrap gap-1.5 text-xs/5 text-fg-muted"
											aria-hidden="true"
										>
											{#each bugParts(r.bugs) as part (part.key)}<SeverityMark
													severity={part.key}
													text={part.text}
												/>{/each}
										</p>
									{/if}
								</div>
								{#if amounts}
									<div class="mt-2 min-w-0 @min-[54rem]:mt-0" data-testid="investment">
										{@render money(r)}
									</div>
								{/if}
							</li>
						{/each}
					</ul>
				</div>
			</div>
		{/if}
	{/snippet}
</Loaded>

<NewProjectSheet
	bind:this={sheet}
	{organizationId}
	oncreated={created}
	onCloseAutoFocus={restoreFocus}
/>

<style>
	.section-header {
		flex: none;
	}
	.section-header:has(.section-toolbar) :global(header) {
		border-bottom: 0;
		padding-block-end: 0.5rem;
	}
	.section-toolbar {
		position: relative;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem 0.75rem;
		padding: 0.375rem 1rem 0.75rem;
		border-block-end: 1px solid var(--line);
	}
	@media (min-width: 640px) {
		.section-toolbar {
			padding-inline: 1.5rem;
		}
	}
	@media (min-width: 1024px) {
		.section-toolbar {
			padding-inline: 2rem;
		}
	}
	.queue-controls {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.25rem;
	}
	.project-filter-btn {
		display: inline-flex;
		align-items: center;
		gap: 0.375rem;
		min-height: 2.25rem;
		padding: 0.25rem 0.75rem;
		border: 1px solid transparent;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: transparent;
		color: var(--exec-body);
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.project-filter-btn:hover {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.project-filter-btn[aria-pressed='true'] {
		background: var(--tone-accent-bg);
		border-color: var(--tone-accent-ring);
		color: var(--exec-ink);
	}
	.project-filter-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.project-filter-cnt {
		color: var(--exec-muted);
		font-size: 0.75rem;
	}
	.project-filter-btn[aria-pressed='true'] .project-filter-cnt {
		color: var(--tone-accent-ink);
	}
	.project-toolbar-actions {
		margin-inline-start: auto;
	}
	@media (max-width: 720px), (pointer: coarse) {
		.project-filter-btn {
			min-height: 2.75rem;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.project-filter-btn {
			transition: none;
		}
	}
	@media (forced-colors: active) {
		.project-filter-btn {
			border: 1px solid ButtonText;
		}
		.project-filter-btn[aria-pressed='true'] {
			border: 2px solid Highlight;
			color: Highlight;
		}
	}
	.projects-masthead :global(h2) {
		font-size: 1.25rem;
		line-height: 1.75rem;
		letter-spacing: -0.02em;
	}
	:global(html[data-ui-theme^='report']) .projects-masthead :global(header) {
		border-bottom: 1.5px solid var(--report-masthead-rule);
	}
	:global(html[data-ui-theme^='report']) .projects-masthead:has(.section-toolbar) :global(header) {
		border-bottom: 0;
	}
	:global(html[data-ui-theme^='report']) .projects-masthead .section-toolbar {
		border-block-end: 1.5px solid var(--report-masthead-rule);
	}
	.proj-kpi-tile {
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: 1rem 1.125rem;
		min-width: 0;
	}
	.proj-kpi-label {
		font-size: 0.7rem;
		font-weight: 650;
		letter-spacing: 0.09em;
		text-transform: uppercase;
		color: var(--exec-muted);
	}
	.proj-kpi-num {
		font-size: 1.5rem;
		line-height: 1.15;
		font-weight: 700;
		letter-spacing: -0.03em;
		color: var(--exec-ink);
		font-variant-numeric: tabular-nums;
	}
	.proj-pill-accent {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
	:global(html[data-ui-theme^='report']) .proj-kpi-strip {
		gap: 0;
		border-bottom: 1px solid var(--report-rule);
		padding-bottom: 0.75rem;
	}
	:global(html[data-ui-theme^='report']) .proj-kpi-tile {
		padding: 0.625rem 0;
	}
	:global(html[data-ui-theme^='report']) .proj-kpi-tile + .proj-kpi-tile {
		border-top: 1px solid var(--report-rule-subtle);
	}
	@media (min-width: 1024px) {
		:global(html[data-ui-theme^='report']) .proj-kpi-tile {
			padding: 0.5rem 1.25rem;
			border-top: 0;
			border-left: 1px solid var(--report-rule-subtle);
		}
		:global(html[data-ui-theme^='report']) .proj-kpi-tile:first-child {
			border-left: 0;
			padding-left: 0;
		}
		:global(html[data-ui-theme^='report']) .proj-kpi-tile + .proj-kpi-tile {
			border-top: 0;
		}
	}
</style>
