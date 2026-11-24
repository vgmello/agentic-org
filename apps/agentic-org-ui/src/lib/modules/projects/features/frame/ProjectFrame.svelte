<!--
  @component
  The page around a project's tabs `/projects/:id/*` (work-items-ux §3, PRJ-1/3): the one named Projects back link; the
  name with its P- reference and slug, the accountable team and any contributors (each a link), and real done/total
  counts; then the tabs as links in the URL, computed from the project and what this release serves (PKG-15). Changing
  tab focuses the tab's heading after the shell's own focus. States: loading (a skeleton shaped like the page),
  unavailable, error with Retry, not found, archived, and the last copy while HQ can't be reached (PRJ-4).
-->
<script lang="ts">
	import { onDestroy, tick, untrack, type Snippet } from 'svelte';
	import { afterNavigate, beforeNavigate, goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { mergeProps, Tooltip } from 'bits-ui';
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import Ellipsis from '@lucide/svelte/icons/ellipsis';
	import FileText from '@lucide/svelte/icons/file-text';
	import Layers from '@lucide/svelte/icons/layers';
	import Package from '@lucide/svelte/icons/package';
	import Settings from '@lucide/svelte/icons/settings';
	import Callout from '$lib/components/Callout.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import TabsNav from '$lib/components/TabsNav.svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Resource } from '$lib/components/resource.svelte';
	import { pageTitle } from '$lib/nav';
	import { projectsApi } from '../../api';
	import {
		PROJECTS_URL,
		countedTasks,
		doneText,
		progressPercent,
		projectBarTone,
		projectHealthLabel,
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
	import { focusHeading, focusHeadingAfterShell } from '$lib/components/focus';
	import { takeArrival } from './arrival';
	import { lookUpProject } from './lookup';
	import FrameSkeleton from './FrameSkeleton.svelte';
	import ProjectReleaseStrip from './ProjectReleaseStrip.svelte';

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

	const MORE_TAB_IDS: Record<string, true> = {
		features: true,
		releases: true,
		docs: true,
		settings: true
	};
	const moreTabs = $derived(tabs.filter((t) => Object.hasOwn(MORE_TAB_IDS, t.id)));
	const morePrimaryTabs = $derived(moreTabs.filter((t) => t.id !== 'settings'));
	const moreSettingsTab = $derived(moreTabs.find((t) => t.id === 'settings'));
	let toolbarControls = $state.raw<Snippet | undefined>(undefined);
	let moreMenuOpen = $state(false);
	const isActivityTab = $derived(tab?.id === 'activity');

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
		reload: () => read.reload(),
		setToolbarControls: (controls) => {
			toolbarControls = controls;
		}
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
	// its destination heading a tick after navigation; this runs strictly after it.
	let cancelTabFocus: (() => void) | undefined;
	onDestroy(() => cancelTabFocus?.());
	beforeNavigate(() => {
		cancelTabFocus?.();
		cancelTabFocus = undefined;
	});
	afterNavigate((move) => {
		cancelTabFocus?.();
		cancelTabFocus = undefined;
		if (move.type === 'enter' || !move.from || !move.to || !project) return;
		const base = projectHref(projectId);
		const inProject = (p: string) => p === base || p.startsWith(`${base}/`);
		if (!inProject(move.from.url.pathname) || !inProject(move.to.url.pathname)) return;
		if (
			tabAt(move.from.url.pathname, projectId, tabs).id ===
			tabAt(move.to.url.pathname, projectId, tabs).id
		)
			return;
		let cancelled = false;
		let timer: ReturnType<typeof setTimeout> | undefined;
		void tick().then(() => {
			if (cancelled) return;
			timer = setTimeout(() => {
				if (cancelled) return;
				const active = document.activeElement;
				const idle =
					!active ||
					active === document.body ||
					active.id === 'shell-page-title' ||
					active.id === 'project-heading' ||
					(active.getAttribute('aria-current') === 'page' &&
						Boolean(active.closest('nav[aria-label$=" views"]')));
				if (idle) focusHeading('project-tab-heading');
			}, 0);
		});
		cancelTabFocus = () => {
			cancelled = true;
			clearTimeout(timer);
		};
	});
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

<svelte:head><title>{title}</title></svelte:head>

<div class={['project-frame-root', isActivityTab && 'project-frame-activity']}>
	<!-- The one named back link, in every state. -->
	<div class="shrink-0 px-4 pt-5 min-[640px]:px-6 min-[1024px]:px-8">
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
					<div class="section-header project-header">
						<header
							class="project-masthead ph px-4 pt-2 pb-3 min-[640px]:px-6 min-[1024px]:px-8"
							data-testid="project-header"
						>
							<h2
								id="project-heading"
								class="flex flex-wrap items-center gap-x-2.5 gap-y-1 text-xl/7 font-semibold tracking-tight text-fg [overflow-wrap:anywhere] focus:outline-none"
							>
								{p.name}
								<span
									class="rounded bg-surface-raised px-1.5 py-0.5 font-mono text-xs/5 font-normal text-fg-muted ring-1 ring-line ring-inset"
									>{projectRef(p)}</span
								>
								<span
									class={[
										'inline-flex shrink-0 items-center rounded-full px-2 py-0.5 text-xs/5 font-semibold whitespace-nowrap',
										healthPillClass(tone)
									]}>{projectHealthLabel(tone)}</span
								>
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
									<span class="shrink-0 text-xs/5 font-semibold text-fg-muted tabular-nums"
										>{pct}%</span
									>
								{/if}
							</div>
						</header>
						<!-- Tabs wrap onto a second line rather than scroll out of sight (narrow widths, large text). -->
						<div class="section-toolbar project-toolbar">
							<TabsNav
								label="{p.name} views"
								tabs={tabs.map((t) => ({
									label: t.label,
									href: projectHref(p.id, t.path),
									match: t.path ? ('prefix' as const) : ('exact' as const)
								}))}
								class="tabs flex-wrap overflow-x-visible"
							/>
							<div class="project-toolbar-controls" role="group" aria-label="Project controls">
								{#if toolbarControls}
									{@render toolbarControls()}
								{/if}
								{#if moreTabs.length > 0}
									<Tooltip.Provider delayDuration={200} ignoreNonKeyboardFocus>
										<DropdownMenu.Root bind:open={moreMenuOpen}>
											<Tooltip.Root
												delayDuration={250}
												ignoreNonKeyboardFocus
												disabled={moreMenuOpen}
											>
												<DropdownMenu.Trigger>
													{#snippet child({ props: menuProps })}
														<Tooltip.Trigger>
															{#snippet child({ props: tipProps })}
																<button
																	{...mergeProps(menuProps, tipProps)}
																	type="button"
																	aria-label="More actions for {p.name}"
																	data-testid="project-more-menu"
																	class="project-more-btn min-h-9 min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
																>
																	<Ellipsis class="size-4" aria-hidden="true" />
																</button>
															{/snippet}
														</Tooltip.Trigger>
													{/snippet}
												</DropdownMenu.Trigger>
												<Tooltip.Portal>
													<Tooltip.Content
														side="bottom"
														sideOffset={6}
														escapeKeydownBehavior="defer-otherwise-close"
														class="project-tooltip-content"
													>
														More actions
													</Tooltip.Content>
												</Tooltip.Portal>
											</Tooltip.Root>
											<DropdownMenu.Content align="end" class="triage-menu-surface w-48">
												{#each morePrimaryTabs as item (item.id)}
													<DropdownMenu.Item
														onSelect={() => void goto(resolve(projectHref(p.id, item.path) as '/'))}
													>
														{#if item.id === 'features'}
															<Layers class="size-4" aria-hidden="true" />
														{:else if item.id === 'releases'}
															<Package class="size-4" aria-hidden="true" />
														{:else}
															<FileText class="size-4" aria-hidden="true" />
														{/if}
														<span>{item.label}</span>
													</DropdownMenu.Item>
												{/each}
												{#if morePrimaryTabs.length > 0 && moreSettingsTab}
													<DropdownMenu.Separator />
												{/if}
												{#if moreSettingsTab}
													<DropdownMenu.Item
														onSelect={() =>
															void goto(resolve(projectHref(p.id, moreSettingsTab.path) as '/'))}
													>
														<Settings class="size-4" aria-hidden="true" />
														<span>{p.viewer.canEdit ? 'Project settings' : 'View settings'}</span>
													</DropdownMenu.Item>
												{/if}
											</DropdownMenu.Content>
										</DropdownMenu.Root>
									</Tooltip.Provider>
								{/if}
							</div>
						</div>
					</div>
					{#if tab?.id === 'overview' || tab?.id === 'activity'}
						<ProjectReleaseStrip
							{organizationId}
							projectId={p.id}
							projectName={p.name}
							fallbackTasks={p.tasks}
						/>
					{/if}
					{#if p.stale}
						<StaleNotice stale={p.stale} retrying={read.reloading} onretry={() => read.reload()} />
					{/if}
					{#if p.archived}
						<div class="px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8">
							<Callout>
								<p class="text-sm/6">
									{p.name} is archived. Its records, reference and history stay readable; it takes no
									new work.
								</p>
							</Callout>
						</div>
					{/if}
					<section
						aria-labelledby="project-tab-heading"
						class={isActivityTab ? 'project-tab-activity' : undefined}
					>
						<h3 id="project-tab-heading" tabindex="-1" class="sr-only">{tab?.label}</h3>
						{@render content()}
					</section>
				{/if}
			{/snippet}
		</Loaded>
	{/if}
</div>

<style>
	.project-frame-root {
		display: flex;
		flex-direction: column;
		min-width: 0;
	}
	.project-tab-activity {
		display: flex;
		flex-direction: column;
		min-width: 0;
	}
	@media (min-height: 640px) {
		.project-frame-activity {
			min-height: 0;
			overflow: hidden;
		}
		.project-tab-activity {
			flex: 1 1 0%;
			min-height: 0;
			overflow: hidden;
		}
	}
	.section-header {
		flex: none;
	}
	.section-toolbar {
		position: relative;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0 12px;
		padding-inline: 1rem;
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
	.section-toolbar > :global(nav) {
		min-width: 0;
		max-width: 100%;
		flex: 1 1 auto;
		padding-inline: 0;
		border-bottom: 0;
		gap: 0.25rem;
	}
	.section-toolbar :global(nav a) {
		padding-inline: 0.625rem;
		border-top-left-radius: var(--exec-ctrl-radius, 6px);
		border-top-right-radius: var(--exec-ctrl-radius, 6px);
	}
	.section-toolbar :global(nav a:hover:not([aria-current='page'])) {
		background: var(--surface-raised);
		border-bottom-color: transparent;
	}
	.section-toolbar :global(nav a[aria-current='page']) {
		border-bottom-color: var(--tone-accent-ink);
		box-shadow: inset 0 -1px var(--tone-accent-ink);
		color: var(--fg);
	}
	.project-toolbar-controls {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
		padding-block: 4px;
		max-width: 100%;
	}
	.project-more-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		border: 0;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: transparent;
		color: var(--fg-secondary);
		cursor: pointer;
	}
	.project-more-btn:hover {
		background: var(--surface-raised);
		color: var(--fg);
	}
	.project-more-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	:global(.project-tooltip-content) {
		z-index: 60;
		padding: 4px 8px;
		border: 1px solid var(--line-strong);
		border-radius: var(--exec-sm-radius, 6px);
		background: var(--side-panel-surface, var(--surface-overlay, var(--exec-surface)));
		color: var(--exec-ink);
		font-size: 0.75rem;
		font-weight: 500;
		line-height: 1.3;
		box-shadow: var(--tooltip-shadow);
		pointer-events: none;
	}
	.proj-pill-accent {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
	:global(html[data-ui-theme^='report']) .project-toolbar {
		border-block-end: 1.5px solid var(--report-masthead-rule);
	}
</style>
