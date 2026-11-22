<!--
  @component
  The page around a project's tabs `/projects/:id/*` (work-items-ux §3, PRJ-1/3): the one named Projects back link; the
  name with its P- reference and slug, the accountable team and any contributors (each a link), and real done/total
  counts; then the tabs as links in the URL, computed from the project and what this release serves (PKG-15). Changing
  tab focuses the tab's heading after the shell's own focus. States: loading (a skeleton shaped like the page),
  unavailable, error with Retry, not found, archived, and the last copy while HQ can't be reached (PRJ-4).
-->
<script lang="ts">
	import { untrack, type Snippet } from 'svelte';
	import { afterNavigate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import Callout from '$lib/components/Callout.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import TabsNav from '$lib/components/TabsNav.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { pageTitle } from '$lib/nav';
	import { projectsApi } from '../../api';
	import {
		PROJECTS_URL,
		countedTasks,
		doneText,
		progressPercent,
		projectBarTone,
		projectHref,
		projectRef,
		projectTabs,
		tabAt,
		type ProjectBarTone
	} from '../../present';
	import { fact, factsLine } from '../../components/classes';
	import TeamLink from '../../components/TeamLink.svelte';
	import StaleNotice from '../../components/StaleNotice.svelte';
	import { setProjectFrame } from './context';
	import { focusHeadingAfterShell } from '$lib/components/focus';
	import { takeArrival } from './arrival';
	import { lookUpProject } from './lookup';
	import FrameSkeleton from './FrameSkeleton.svelte';

	let {
		organizationId,
		projectId,
		content
	}: {
		organizationId: string;
		projectId: string;
		/** The current tab; it reads the project from the frame's context. */
		content: Snippet;
	} = $props();

	const read = new Resource(() =>
		lookUpProject(() => projectsApi.project(organizationId, projectId))
	);
	const project = $derived(read.data?.found ? read.data.project : undefined);
	const tabs = $derived(project ? projectTabs(project) : []);
	const tab = $derived(project ? tabAt(page.url.pathname, projectId, tabs) : undefined);
	const title = $derived(
		read.state !== 'ready'
			? pageTitle('Projects')
			: project && tab
				? pageTitle(tab.id === 'overview' ? undefined : tab.label, project.name, 'Projects')
				: pageTitle('Project not found', 'Projects')
	);

	setProjectFrame({
		get organizationId() {
			return organizationId;
		},
		get projectId() {
			return projectId;
		},
		get project() {
			return project!;
		},
		reload: () => read.reload()
	});

	// Arriving from New project: the heading takes focus once the project is there, after the shell's own focus. An
	// effect, because moving DOM focus is a side effect of the read landing; no state is written.
	// The route keys the frame by project, so the first project ID is the only one.
	const arrived = untrack(() => takeArrival(projectId));
	$effect(() => {
		if (!arrived || read.state === 'loading') return;
		return focusHeadingAfterShell('project-heading');
	});

	// Changing tab moves focus to the tab's heading, so what was shown is announced (app-shell §0). The shell focuses
	// its h1 a tick after a navigation; this runs after it.
	afterNavigate((move) => {
		if (move.type === 'enter' || !move.from || !move.to || !project) return;
		const base = projectHref(projectId);
		const inProject = (p: string) => p === base || p.startsWith(`${base}/`);
		if (!inProject(move.from.url.pathname) || !inProject(move.to.url.pathname)) return;
		if (
			tabAt(move.from.url.pathname, projectId, tabs).id ===
			tabAt(move.to.url.pathname, projectId, tabs).id
		)
			return;
		setTimeout(() => document.getElementById('project-tab-heading')?.focus(), 0);
	});
	const barFill: Record<ProjectBarTone, string> = {
		ok: 'var(--tone-ok-bar)',
		warn: 'var(--tone-warn-bar)',
		bad: 'var(--tone-bad-bar)',
		accent: 'var(--tone-accent-bar)',
		archived: 'var(--quiet)'
	};
</script>

<svelte:head><title>{title}</title></svelte:head>

<!-- The one named back link, in every state. -->
<div class="px-4 pt-5 min-[640px]:px-6 min-[1024px]:px-8">
	<a
		href={resolve(PROJECTS_URL)}
		class="inline-flex min-h-6 items-center gap-1 rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
		><ChevronLeft class="size-4 shrink-0" aria-hidden="true" />Projects</a
	>
</div>
{#if read.state === 'loading'}
	<FrameSkeleton />
{:else}
	<Loaded resource={read} label="the project" error="Couldn't load this project">
		{#snippet children(lookup)}
			{#if !lookup.found}
				<header class="border-b border-line px-4 pt-2 pb-6 min-[640px]:px-6 min-[1024px]:px-8">
					<h2
						id="project-heading"
						class="text-xl/7 font-semibold tracking-tight text-fg [overflow-wrap:anywhere] focus:outline-none"
					>
						Project not found
					</h2>
					<p class="mt-1 max-w-3xl text-sm/6 text-fg-muted">{lookup.message}</p>
				</header>
			{:else}
				{@const p = lookup.project}
				{@const totalTasks = countedTasks(p.tasks)}
				{@const pct = progressPercent(p.tasks)}
				{@const tone = projectBarTone(p.tasks, p.bugs, p.archived)}
				<header
					class="project-masthead border-b border-line px-4 pt-2 pb-6 min-[640px]:px-6 min-[1024px]:px-8"
					data-testid="project-header"
				>
					<h2
						id="project-heading"
						class="flex flex-wrap items-baseline gap-x-2.5 gap-y-1 text-xl/7 font-semibold tracking-tight text-fg [overflow-wrap:anywhere] focus:outline-none"
					>
						{p.name}
						<span
							class="rounded bg-surface-raised px-1.5 py-0.5 font-mono text-xs/5 font-normal text-fg-muted ring-1 ring-line ring-inset"
							>{projectRef(p)}</span
						>
						{#if p.archived}<span
								class="inline-flex shrink-0 items-center rounded-full bg-surface-raised px-2 py-0.5 text-xs/5 font-medium whitespace-nowrap text-fg-secondary ring-1 ring-line ring-inset"
								>Archived</span
							>{/if}
					</h2>
					<ul role="list" class="mt-2 {factsLine} text-sm/6 text-fg-muted">
						<li class={fact} data-testid="owner-team">
							Accountable team: <TeamLink team={p.ownerTeam} />
						</li>
						{#if p.contributors.length > 0}
							<li class={fact} data-testid="contributors">
								Contributors: {#each p.contributors as t, i (t.id)}{#if i > 0},
									{/if}<TeamLink team={t} />{/each}
							</li>
						{/if}
						<li class="{fact} tabular-nums">{doneText(p.tasks)}</li>
						<li class={fact}>
							Slug: <span class="font-mono text-xs/5 text-fg-secondary">{p.slug}</span>
						</li>
					</ul>
					<div class="mt-3 flex max-w-sm items-center gap-3">
						<div
							class="h-1.5 flex-1 overflow-hidden rounded-full bg-bar-track"
							role="progressbar"
							aria-label="{p.name} delivery progress"
							aria-valuemin="0"
							aria-valuemax={totalTasks}
							aria-valuenow={p.tasks.done}
							aria-valuetext={doneText(p.tasks)}
						>
							<span
								class="block h-full rounded-full transition-[width] duration-300 motion-reduce:transition-none"
								style:width="{pct}%"
								style:background={barFill[tone]}
							></span>
						</div>
						{#if totalTasks > 0}
							<span class="shrink-0 text-xs/5 font-medium text-fg-muted tabular-nums">{pct}%</span>
						{/if}
					</div>
				</header>
				{#if p.stale}
					<StaleNotice stale={p.stale} retrying={read.reloading} onretry={() => read.reload()} />
				{/if}
				{#if p.archived}
					<div class="px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8">
						<Callout>
							<p class="text-sm/6">
								{p.name} is archived. Its records, reference and history stay readable; it takes no new
								work.
							</p>
						</Callout>
					</div>
				{/if}
				<!-- Tabs wrap onto a second line rather than scroll out of sight (narrow widths, large text). -->
				<TabsNav
					label="{p.name} views"
					tabs={tabs.map((t) => ({
						label: t.label,
						href: projectHref(p.id, t.path),
						match: t.path ? ('prefix' as const) : ('exact' as const)
					}))}
					class="flex-wrap overflow-x-visible"
				/>
				<section aria-labelledby="project-tab-heading">
					<h3 id="project-tab-heading" tabindex="-1" class="sr-only">{tab?.label}</h3>
					{@render content()}
				</section>
			{/if}
		{/snippet}
	</Loaded>
{/if}
