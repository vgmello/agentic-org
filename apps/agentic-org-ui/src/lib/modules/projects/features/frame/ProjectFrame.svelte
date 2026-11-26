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
	import Ellipsis from '@lucide/svelte/icons/ellipsis';
	import FileText from '@lucide/svelte/icons/file-text';
	import Layers from '@lucide/svelte/icons/layers';
	import Package from '@lucide/svelte/icons/package';
	import Settings from '@lucide/svelte/icons/settings';
	import Breadcrumbs from '$lib/components/Breadcrumbs.svelte';
	import Callout from '$lib/components/Callout.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import InteriorSearchBar from '$lib/platform/shell/InteriorSearchBar.svelte';
	import TabsNav from '$lib/components/TabsNav.svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Resource } from '$lib/components/resource.svelte';
	import { pageTitle } from '$lib/nav';
	import { projectsApi } from '../../api';
	import {
		PROJECTS_URL,
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
	<!-- Breadcrumb trail, in every state. -->
	<div class="shrink-0 px-4 pt-5 min-[640px]:px-6 min-[1024px]:px-8">
		<Breadcrumbs
			items={[
				{ label: 'Projects', href: PROJECTS_URL },
				{ label: project?.name ?? (read.state === 'loading' ? 'Loading…' : 'Project') }
			]}
		/>
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
					{@const tone = projectBarTone(p.tasks, p.bugs, p.archived)}
					<div class="section-header project-header">
						<header
							class="project-masthead ph flex flex-wrap items-start justify-between gap-x-6 gap-y-3 px-4 pt-2 pb-2 min-[640px]:px-6 min-[1024px]:px-8"
							data-testid="project-header"
						>
							<div class="min-w-0 flex-1 basis-64">
								<h2
									id="project-heading"
									tabindex="-1"
									class="flex flex-wrap items-center gap-x-2.5 gap-y-1 text-[clamp(1.375rem,2.2vw,1.875rem)]/[1.22] font-semibold tracking-[-0.03em] text-fg [overflow-wrap:anywhere] focus:outline-none"
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
									<li class={fact}>
										Slug: <span class="font-mono text-xs/5 text-fg-secondary">{p.slug}</span>
									</li>
								</ul>
							</div>
							<InteriorSearchBar class="w-full shrink-0 sm:w-[clamp(18rem,28vw,25rem)]" />
						</header>
						<!-- Tabs wrap onto a second line rather than scroll out of sight (narrow widths, large text). -->
						<div class="section-toolbar project-toolbar">
							<div class="primary-tab-row">
								<TabsNav
									label="{p.name} views"
									tabs={tabs.map((t) => ({
										label: t.label,
										href: projectHref(p.id, t.path),
										match: t.path ? ('prefix' as const) : ('exact' as const)
									}))}
									class="tabs flex-wrap overflow-x-visible"
								/>
							</div>
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
		position: relative;
		z-index: 20;
		flex: none;
		container-type: inline-size;
		--proj-gutter: 1rem;
	}
	@media (min-width: 640px) {
		.section-header {
			--proj-gutter: 1.5rem;
		}
	}
	@media (min-width: 1024px) {
		.section-header {
			--proj-gutter: 2rem;
		}
	}
	.section-toolbar {
		position: relative;
		display: flex;
		flex-wrap: wrap;
		align-items: stretch;
		gap: 6px 12px;
		padding: 4px var(--proj-gutter) 1px;
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
		flex: 0 1 auto;
	}
	.primary-tab-row::after {
		content: '';
		position: absolute;
		inset-block-start: 100%;
		inset-inline-start: calc(-1 * var(--proj-gutter));
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
	.project-toolbar-controls {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
		margin-block: 4px;
		max-width: 100%;
	}
	@container (max-width: 40rem) {
		.primary-tab-row {
			flex-basis: 100%;
		}
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
