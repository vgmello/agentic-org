<script lang="ts">
	import { resolve } from '$app/paths';
	import { Tooltip } from 'bits-ui';
	import Activity from '@lucide/svelte/icons/activity';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import Layers from '@lucide/svelte/icons/layers';
	import InteriorSearchBar from '$lib/components/shell/InteriorSearchBar.svelte';
	import type { ProjectSummary } from '$lib/proto';
	import ActivityDisplayMenu from './ActivityDisplayMenu.svelte';
	import { STREAM_TABS } from './activity-project-merge';
	import type { FeedPhase, StreamFilterTab } from './assembly-model';

	let {
		headerSubtitle,
		shellReady,
		workUnavailableReason = undefined,
		inboxPhase,
		isHeaderRetrying,
		needsYouCount,
		streamNeedsYouCount,
		activeProjects,
		effectiveProjectScope,
		viewMode = $bindable<'assembly' | 'feed'>('assembly'),
		streamFilter = $bindable<StreamFilterTab>('all'),
		displayMenuOpen = $bindable(false),
		onProjectScopeChange,
		onHeaderRetry,
		onViewKeydown
	}: {
		headerSubtitle: string;
		shellReady: boolean;
		workUnavailableReason?: string;
		inboxPhase: FeedPhase;
		isHeaderRetrying: boolean;
		needsYouCount: number;
		streamNeedsYouCount: number;
		activeProjects: readonly ProjectSummary[];
		effectiveProjectScope: string;
		viewMode?: 'assembly' | 'feed';
		streamFilter?: StreamFilterTab;
		displayMenuOpen?: boolean;
		onProjectScopeChange: (scope: string) => void;
		onHeaderRetry: (event: MouseEvent) => void;
		onViewKeydown: (event: KeyboardEvent) => void;
	} = $props();

	function handleStreamTabKeydown(event: KeyboardEvent, index: number) {
		let nextIndex: number;
		if (event.key === 'ArrowRight') nextIndex = (index + 1) % STREAM_TABS.length;
		else if (event.key === 'ArrowLeft')
			nextIndex = (index - 1 + STREAM_TABS.length) % STREAM_TABS.length;
		else if (event.key === 'Home') nextIndex = 0;
		else if (event.key === 'End') nextIndex = STREAM_TABS.length - 1;
		else return;

		event.preventDefault();
		streamFilter = STREAM_TABS[nextIndex].id;
		const tablist = (event.currentTarget as HTMLElement | null)?.closest('[role="tablist"]');
		tablist?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[nextIndex]?.focus();
	}
</script>

<div class="section-header activity-header">
	<header class="ph cmd-header">
		<div class="ph-t cmd-title-group">
			<h1 id="shell-page-title" tabindex="-1" class="cmd-h1 focus:outline-none">Activity</h1>
			<p class="cmd-subtitle">{headerSubtitle}</p>
		</div>

		<div class="cmd-status-slot flex flex-wrap items-center gap-3" tabindex="-1">
			{#if workUnavailableReason || inboxPhase === 'error' || isHeaderRetrying}
				<button
					type="button"
					disabled={isHeaderRetrying}
					aria-busy={isHeaderRetrying}
					onclick={onHeaderRetry}
					class="cmd-needs-pill is-warn min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
				>
					<AlertTriangle class="size-3.5" aria-hidden="true" />
					<span>
						{#if isHeaderRetrying}
							Retrying…
						{:else if workUnavailableReason}
							Retry work load
						{:else}
							Inbox unavailable
						{/if}
					</span>
				</button>
			{:else if !shellReady || inboxPhase === 'loading'}
				<span
					class="cmd-needs-pill is-neutral min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
				>
					<span>Loading status…</span>
				</span>
			{:else if needsYouCount > 0}
				<a
					href={resolve('/inbox')}
					class="cmd-needs-pill is-warn min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
				>
					<span class="cmd-needs-dot" aria-hidden="true"></span>
					<span class="tabular-nums">{needsYouCount} need you</span>
				</a>
			{:else}
				<span
					class="cmd-needs-pill is-ok min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
				>
					<CheckCircle2 class="size-3.5" aria-hidden="true" />
					<span>All caught up</span>
				</span>
			{/if}
			<InteriorSearchBar class="w-full sm:w-[clamp(18rem,28vw,25rem)]" />
		</div>
	</header>

	<Tooltip.Provider delayDuration={200} ignoreNonKeyboardFocus>
		<div class="section-toolbar activity-toolbar">
			<div class="primary-tab-row" data-page-menu>
				<div class="activity-view-tabs" role="group" aria-label="Activity view">
					<button
						type="button"
						data-menu-label="Assembly line"
						data-project-view="assembly"
						aria-pressed={viewMode === 'assembly'}
						onclick={() => (viewMode = 'assembly')}
						onkeydown={onViewKeydown}
						class={[
							'cmd-toggle-btn queue-tab min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11',
							viewMode === 'assembly' && 'is-on'
						]}
					>
						<Layers class="size-3.5" aria-hidden="true" />
						<span>Assembly line</span>
					</button>
					<button
						type="button"
						data-menu-label="Activity"
						data-project-view="feed"
						aria-pressed={viewMode === 'feed'}
						onclick={() => (viewMode = 'feed')}
						onkeydown={onViewKeydown}
						class={[
							'cmd-toggle-btn queue-tab min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11',
							viewMode === 'feed' && 'is-on'
						]}
					>
						<Activity class="size-3.5" aria-hidden="true" />
						<span>Activity</span>
					</button>
				</div>
			</div>

			<div class="cmd-select-wrap activity-scope">
				<select
					aria-label="Project scope"
					value={effectiveProjectScope}
					onchange={(event) => onProjectScopeChange(event.currentTarget.value)}
					class="cmd-select min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
				>
					<option value="all">
						All projects ({activeProjects.length} active)
					</option>
					{#each activeProjects as project (project.id)}
						<option value={project.id}>
							{project.name}{#if project.key}
								· {project.key}{/if}
						</option>
					{/each}
				</select>
				<ChevronDown class="cmd-select-chevron size-3.5" aria-hidden="true" />
			</div>

			{#if viewMode === 'assembly'}
				<div class="activity-toolbar-end">
					<ActivityDisplayMenu bind:open={displayMenuOpen} />
				</div>
			{:else}
				<div class="tabs activity-stream-tabs" role="tablist" aria-label="Filter activity stream">
					{#each STREAM_TABS as tab, index (tab.id)}
						<button
							type="button"
							role="tab"
							id={`activity-tab-${tab.id}`}
							tabindex={streamFilter === tab.id ? 0 : -1}
							aria-selected={streamFilter === tab.id}
							aria-controls={`activity-panel-${tab.id}`}
							onclick={() => (streamFilter = tab.id)}
							onkeydown={(event) => handleStreamTabKeydown(event, index)}
							class={[
								'activity-stream-tab min-h-9 min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11',
								streamFilter === tab.id && 'is-active'
							]}
						>
							<span>{tab.label}</span>
							{#if tab.id === 'needs_you' && streamNeedsYouCount > 0}
								<span class="activity-stream-badge tabular-nums">{streamNeedsYouCount}</span>
							{/if}
						</button>
					{/each}
				</div>
			{/if}
		</div>
	</Tooltip.Provider>
</div>

<style>
	.section-header {
		position: relative;
		z-index: 1;
		flex: none;
		container-type: inline-size;
	}
	.cmd-header {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 10px 20px;
		padding: 14px var(--activity-gutter) 8px;
	}
	.cmd-title-group {
		min-width: 0;
	}
	.cmd-h1 {
		color: var(--exec-ink);
		font-size: clamp(1.375rem, 2.2vw, 1.875rem);
		font-weight: 600;
		letter-spacing: -0.03em;
		line-height: 1.22;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.cmd-subtitle {
		margin-top: 4px;
		color: var(--fg-secondary);
		font-size: 0.875rem;
		line-height: 1.4;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.section-toolbar {
		position: relative;
		display: flex;
		flex-wrap: wrap;
		align-items: stretch;
		gap: 6px 12px;
		padding: 4px var(--activity-gutter) 1px;
		border-block-end: 1px solid var(--line);
	}
	.section-toolbar > :not(.primary-tab-row) {
		align-self: center;
		margin-block: 4px;
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
		inset-inline-start: calc(-1 * var(--activity-gutter));
		inline-size: 100cqi;
		border-block-start: 1px solid var(--line);
		pointer-events: none;
	}
	.activity-view-tabs {
		display: flex;
		align-items: stretch;
		gap: 4px;
	}
	.activity-toolbar-end {
		display: flex;
		align-items: center;
		margin-inline-start: auto;
	}
	.activity-stream-tabs {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 4px 12px;
		min-width: 0;
		max-width: 100%;
		padding-inline: 0;
		box-shadow: none;
	}
	.activity-stream-tab {
		position: relative;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 6px;
		padding: 0 8px;
		border: 0;
		border-bottom: 2px solid transparent;
		background: transparent;
		color: var(--fg-secondary);
		font-size: 0.8125rem;
		font-weight: 550;
		white-space: nowrap;
		cursor: pointer;
	}
	.activity-stream-tab:hover {
		color: var(--exec-ink);
	}
	.activity-stream-tab:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	.activity-stream-tab.is-active {
		color: var(--exec-ink);
		border-bottom-color: var(--tone-accent);
	}
	.activity-stream-badge {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: 18px;
		padding: 1px 6px;
		border-radius: var(--exec-pill-radius, 999px);
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
		color: var(--exec-warn);
		font-size: 0.6875rem;
		font-weight: 650;
	}
	.cmd-select-wrap {
		position: relative;
		display: inline-flex;
		align-items: center;
		max-width: 100%;
	}
	.cmd-select {
		appearance: none;
		max-width: 100%;
		padding: 0 32px 0 12px;
		border: 1px solid var(--line-strong);
		border-radius: var(--exec-ctrl-radius, 8px);
		background-color: var(--exec-well);
		color: var(--exec-ink);
		font-size: 0.8125rem;
		font-weight: 550;
		cursor: pointer;
	}
	.cmd-select:hover {
		background-color: var(--exec-surface-hi);
	}
	.cmd-select:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}
	.cmd-select-wrap :global(.cmd-select-chevron) {
		position: absolute;
		right: 10px;
		color: var(--fg-secondary);
		pointer-events: none;
	}
	.cmd-toggle-btn {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 6px 12px 5px;
		border: 1px solid transparent;
		border-block-end-width: 2px;
		border-radius: var(--exec-ctrl-radius, 8px) var(--exec-ctrl-radius, 8px) 0 0;
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.875rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
	}
	.cmd-toggle-btn:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
		color: var(--exec-ink);
	}
	.cmd-toggle-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -4px;
	}
	.cmd-toggle-btn.is-on {
		border-block-end-color: var(--tone-accent-ink);
		color: var(--exec-ink);
	}
	.cmd-status-slot {
		display: inline-flex;
		border-radius: var(--exec-pill-radius, 999px);
	}
	.cmd-status-slot:focus {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.cmd-needs-pill {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		padding: 0 12px;
		border: 0;
		border-radius: var(--exec-pill-radius, 999px);
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		text-decoration: none;
	}
	button.cmd-needs-pill {
		cursor: pointer;
	}
	.cmd-needs-pill.is-neutral {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		color: var(--fg-secondary);
	}
	.cmd-needs-pill.is-warn {
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
		color: var(--exec-warn);
	}
	.cmd-needs-pill.is-warn:hover {
		box-shadow: inset 0 0 0 1.5px var(--exec-warn);
	}
	.cmd-needs-pill.is-ok {
		background: var(--exec-ok-bg);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
		color: var(--exec-ok);
	}
	.cmd-needs-pill:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.cmd-needs-dot {
		width: 7px;
		height: 7px;
		border-radius: 999px;
		background: var(--exec-warn);
		flex-shrink: 0;
	}
	@media (width < 40rem) {
		.cmd-select-wrap {
			flex: 1 1 100%;
		}
		.cmd-select {
			width: 100%;
		}
	}
	:global(html[data-ui-theme^='report']) .activity-toolbar {
		border-block-end: 1.5px solid var(--report-masthead-rule);
	}
	@media (forced-colors: active) {
		.cmd-toggle-btn {
			border-block-end-color: Canvas;
		}
		.cmd-toggle-btn.is-on,
		.activity-stream-tab.is-active {
			border-block-end-color: Highlight;
		}
		.cmd-needs-pill {
			border: 1px solid CanvasText;
		}
	}
</style>
