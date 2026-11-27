<!--
  @component
  Sidebar "Favorite projects" section: honest empty state when none are starred,
  health-toned icons (or neutral folder while offline), Overview links, and unpin star controls.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { Tooltip } from 'bits-ui';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Folder from '@lucide/svelte/icons/folder';
	import Star from '@lucide/svelte/icons/star';
	import XCircle from '@lucide/svelte/icons/x-circle';
	import { tooltipContentClass } from '$lib/components/classes';
	import type { ProjectSummary, ProjectTeam, WorkItemSummary } from '$lib/proto';
	import type { InboxItem } from '$lib/platform/workspace/inbox-types';
	import { uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';
	import { deriveProjectHealth } from './project-health';

	let {
		projects = [],
		workItems = [],
		inboxItems = [],
		projectTeams,
		offline = false,
		isActive,
		onNavigate
	}: {
		projects?: ProjectSummary[];
		workItems?: readonly WorkItemSummary[];
		inboxItems?: readonly InboxItem[];
		projectTeams?: readonly ProjectTeam[];
		offline?: boolean;
		isActive: (href: string) => boolean;
		onNavigate?: (event: MouseEvent, href: string) => void;
	} = $props();

	let favoritesOpen = $state(true);
	const favoriteList = $derived.by(() => {
		const byId = new Map(projects.filter((p) => !p.archived).map((p) => [p.id, p]));
		return uiPrefs.favoriteProjects
			.map((id) => byId.get(id))
			.filter((p): p is ProjectSummary => p !== undefined);
	});
</script>

<div data-testid="favorite-projects-section">
	<div class="flex items-center justify-between gap-1 px-2.5 pb-1.5">
		<div class="exec-nav-label flex items-center gap-1.5">
			<Star class="size-3" aria-hidden="true" />
			<span>Favorite projects</span>
		</div>
		<Tooltip.Root>
			<Tooltip.Trigger>
				{#snippet child({ props })}
					<button
						{...props}
						type="button"
						onclick={() => (favoritesOpen = !favoritesOpen)}
						aria-expanded={favoritesOpen}
						aria-controls="exec-nav-favs"
						aria-label={favoritesOpen ? 'Collapse favorite projects' : 'Expand favorite projects'}
						class="exec-icon-btn flex size-9 items-center justify-center rounded-md [@media(pointer:coarse)]:size-11 [@media(width<=720px)]:size-11"
					>
						<ChevronRight
							class={[
								'size-3.5 transition-transform duration-150 motion-reduce:transition-none',
								favoritesOpen && 'rotate-90'
							]}
							aria-hidden="true"
						/>
					</button>
				{/snippet}
			</Tooltip.Trigger>
			<Tooltip.Portal>
				<Tooltip.Content side="right" sideOffset={6} class={tooltipContentClass}
					>{favoritesOpen
						? 'Collapse favorite projects'
						: 'Expand favorite projects'}</Tooltip.Content
				>
			</Tooltip.Portal>
		</Tooltip.Root>
	</div>
	{#if favoritesOpen}
		<ul id="exec-nav-favs" aria-label="Favorite projects" class="flex flex-col gap-0.5">
			{#if favoriteList.length === 0}
				<li class="exec-fav-empty px-2.5 py-1 text-xs">No favorite projects yet.</li>
			{:else}
				{#each favoriteList as proj (proj.id)}
					{@const href = `/projects/${proj.id}`}
					{@const health = deriveProjectHealth(proj.id, workItems, inboxItems, projectTeams)}
					<li class="group flex items-center gap-0.5">
						<a
							href={resolve(href as '/projects')}
							onclick={(e) => onNavigate?.(e, href)}
							aria-current={isActive(href) ? 'page' : undefined}
							title="{proj.name} · {offline ? 'Offline' : health.label}"
							class={['exec-nav-item min-w-0 flex-1', isActive(href) && 'is-active']}
						>
							{#if offline}
								<Folder class="size-4 shrink-0" aria-hidden="true" />
							{:else if health.tone === 'bad'}
								<XCircle
									class="exec-fav-health is-bad size-4 shrink-0"
									data-health-tone="bad"
									aria-hidden="true"
								/>
							{:else if health.tone === 'warn'}
								<AlertTriangle
									class="exec-fav-health is-warn size-4 shrink-0"
									data-health-tone="warn"
									aria-hidden="true"
								/>
							{:else}
								<CheckCircle2
									class="exec-fav-health is-ok size-4 shrink-0"
									data-health-tone="ok"
									aria-hidden="true"
								/>
							{/if}
							<span class="truncate">{proj.name}</span>
						</a>
						<Tooltip.Root>
							<Tooltip.Trigger>
								{#snippet child({ props })}
									<button
										{...props}
										type="button"
										onclick={() => uiPrefs.toggleFavorite(proj.id)}
										aria-label="Remove {proj.name} from favorites"
										class="exec-fav-unpin flex size-9 shrink-0 items-center justify-center rounded-md opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 [@media(pointer:coarse)]:size-11 [@media(pointer:coarse)]:opacity-100 [@media(width<=720px)]:size-11"
									>
										<Star class="size-3.5 fill-current" aria-hidden="true" />
									</button>
								{/snippet}
							</Tooltip.Trigger>
							<Tooltip.Portal>
								<Tooltip.Content side="right" sideOffset={6} class={tooltipContentClass}
									>Unpin from favorites</Tooltip.Content
								>
							</Tooltip.Portal>
						</Tooltip.Root>
					</li>
				{/each}
			{/if}
		</ul>
	{/if}
</div>

<style>
	.exec-nav-label {
		font-size: var(--exec-nav-label-size);
		line-height: 1.125rem;
		font-weight: 650;
		letter-spacing: var(--exec-nav-label-tracking);
		text-transform: uppercase;
		color: var(--exec-dim);
	}
	.exec-icon-btn {
		color: var(--exec-muted);
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.exec-icon-btn:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.exec-fav-empty {
		color: var(--exec-dim);
	}
	.exec-fav-unpin {
		color: var(--tone-warn);
	}
	.exec-fav-unpin:hover {
		color: var(--tone-warn-ink);
		background: var(--tone-warn-bg);
	}
	.exec-nav-item {
		position: relative;
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 7px 10px;
		border-radius: 8px;
		font-size: 0.875rem;
		line-height: 1.35rem;
		font-weight: 500;
		color: var(--exec-muted);
		white-space: nowrap;
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.exec-nav-item:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.exec-nav-item.is-active {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		font-weight: 600;
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.exec-nav-item.is-active :global(svg:not(.exec-fav-health)) {
		color: var(--exec-accent-ink);
	}
	.exec-nav-item :global(.exec-fav-health.is-ok) {
		color: var(--exec-ok);
	}
	.exec-nav-item :global(.exec-fav-health.is-warn) {
		color: var(--exec-warn);
	}
	.exec-nav-item :global(.exec-fav-health.is-bad) {
		color: var(--exec-bad);
	}
	@media (prefers-reduced-motion: reduce) {
		.exec-icon-btn,
		.exec-nav-item,
		.exec-fav-unpin {
			transition: none;
		}
	}
</style>
