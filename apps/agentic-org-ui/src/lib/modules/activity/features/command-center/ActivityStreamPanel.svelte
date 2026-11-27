<script lang="ts">
	import { SvelteSet } from 'svelte/reactivity';
	import { resolve } from '$app/paths';
	import Activity from '@lucide/svelte/icons/activity';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Inbox from '@lucide/svelte/icons/inbox';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import { relativeAge } from '$lib/format';
	import { accountZone, eventTime } from '$lib/modules/preferences';
	import ActivityStreamBottleneck from './ActivityStreamBottleneck.svelte';
	import ActivityStreamRow from './ActivityStreamRow.svelte';
	import {
		filterStreamEntries,
		groupStreamEntriesByDay,
		type BottleneckSummary,
		type FeedPhase,
		type StreamEntry,
		type StreamFilterTab
	} from './assembly-model';

	let {
		entries,
		decisionEntries = [],
		needsYouCount,
		openInboxCount = 0,
		actionableWorkCount = 0,
		bottleneck,
		activeFilter = $bindable<StreamFilterTab>('all'),
		nowMs,
		feedPhase = 'ready',
		feedError = undefined,
		feedMore = 0,
		decisionPhase = 'ready',
		decisionError = undefined,
		decisionMore = 0,
		inboxPhase = 'ready',
		workPhase = 'ready',
		showTabs = true,
		projectScope = false,
		onRetry = undefined,
		onRetryDecisions = undefined
	}: {
		entries: readonly StreamEntry[];
		decisionEntries?: readonly StreamEntry[];
		needsYouCount: number;
		openInboxCount?: number;
		actionableWorkCount?: number;
		bottleneck: BottleneckSummary;
		activeFilter?: StreamFilterTab;
		nowMs: number;
		feedPhase?: FeedPhase;
		feedError?: string;
		feedMore?: number;
		decisionPhase?: FeedPhase;
		decisionError?: string;
		decisionMore?: number;
		inboxPhase?: FeedPhase;
		workPhase?: FeedPhase;
		showTabs?: boolean;
		projectScope?: boolean;
		onRetry?: () => void;
		onRetryDecisions?: () => void;
	} = $props();

	const workReady = $derived(workPhase === 'ready');

	const tabs: { id: StreamFilterTab; label: string }[] = [
		{ id: 'all', label: 'All' },
		{ id: 'needs_you', label: 'Needs you' },
		{ id: 'work', label: 'Work' },
		{ id: 'run', label: 'Runs' },
		{ id: 'decision', label: 'Decisions' },
		{ id: 'conversation', label: 'Chat' },
		{ id: 'org', label: 'Org' }
	];

	const isDecisionTab = $derived(activeFilter === 'decision');
	const activePhase = $derived(isDecisionTab ? decisionPhase : feedPhase);
	const activeError = $derived(isDecisionTab ? decisionError : feedError);
	const activeMore = $derived(isDecisionTab ? decisionMore : feedMore);
	const filteredEntries = $derived(
		isDecisionTab ? [...decisionEntries] : filterStreamEntries(entries, activeFilter)
	);
	const nowDate = $derived(new Date(nowMs));
	const dayGroups = $derived(groupStreamEntriesByDay(filteredEntries, nowMs, accountZone.zone));

	function entryTimestamp(at: string) {
		const ms = Date.parse(at);
		if (Number.isNaN(ms)) return { text: relativeAge(at, nowMs), full: at, iso: undefined };
		return eventTime(new Date(ms), nowDate, accountZone.zone, true);
	}

	const hasPartialEntries = $derived(!isDecisionTab && entries.length > 0);
	const showEmptyInboxShortcut = $derived(
		activePhase === 'ready' &&
			activeFilter === 'needs_you' &&
			filteredEntries.length === 0 &&
			openInboxCount > 0
	);
	const showEmptyWorkShortcut = $derived(
		activePhase === 'ready' &&
			activeFilter === 'needs_you' &&
			filteredEntries.length === 0 &&
			openInboxCount === 0 &&
			actionableWorkCount > 0
	);

	const expandedRuns = new SvelteSet<string>();

	function toggleRunLog(id: string) {
		if (expandedRuns.has(id)) expandedRuns.delete(id);
		else expandedRuns.add(id);
	}

	function handleRetryClick(event: MouseEvent) {
		const panel = (event.currentTarget as HTMLElement | null)?.closest<HTMLElement>('.stream-body');
		panel?.focus();
		if (isDecisionTab) onRetryDecisions?.();
		else onRetry?.();
	}

	function handleTabKeydown(event: KeyboardEvent, index: number) {
		let nextIndex: number;
		if (event.key === 'ArrowRight') nextIndex = (index + 1) % tabs.length;
		else if (event.key === 'ArrowLeft') nextIndex = (index - 1 + tabs.length) % tabs.length;
		else if (event.key === 'Home') nextIndex = 0;
		else if (event.key === 'End') nextIndex = tabs.length - 1;
		else return;

		event.preventDefault();
		activeFilter = tabs[nextIndex].id;
		const tablist = (event.currentTarget as HTMLElement | null)?.closest('[role="tablist"]');
		tablist?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[nextIndex]?.focus();
	}
</script>

<section class="stream-panel activity-feed" aria-label="Chronological activity stream">
	{#if showTabs}
		<div class="stream-tabs-wrap">
			<div class="stream-tabs" role="tablist" aria-label="Filter activity stream">
				{#each tabs as tab, index (tab.id)}
					<button
						type="button"
						role="tab"
						id={`activity-tab-${tab.id}`}
						tabindex={activeFilter === tab.id ? 0 : -1}
						aria-selected={activeFilter === tab.id}
						aria-controls={`activity-panel-${tab.id}`}
						onclick={() => (activeFilter = tab.id)}
						onkeydown={(event) => handleTabKeydown(event, index)}
						class={[
							'stream-tab min-h-9 min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11',
							activeFilter === tab.id && 'is-active'
						]}
					>
						<span>{tab.label}</span>
						{#if tab.id === 'needs_you' && needsYouCount > 0}
							<span class="stream-tab-badge tabular-nums">{needsYouCount}</span>
						{/if}
					</button>
				{/each}
			</div>
		</div>
	{/if}

	{#if workReady}
		<ActivityStreamBottleneck {bottleneck} />
	{/if}

	{#if !projectScope}
		{#each tabs as tab (tab.id)}
			{#if tab.id !== activeFilter}
				<div
					id={`activity-panel-${tab.id}`}
					role="tabpanel"
					aria-labelledby={`activity-tab-${tab.id}`}
					hidden
				></div>
			{/if}
		{/each}
	{/if}

	<!-- Active Stream Panel (tabindex="0" keeps leading non-focusable panel text keyboard-reachable) -->
	<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
	<div
		id={projectScope ? 'project-activity-stream-body' : `activity-panel-${activeFilter}`}
		role={projectScope ? 'region' : 'tabpanel'}
		aria-labelledby={projectScope ? undefined : `activity-tab-${activeFilter}`}
		aria-label={projectScope ? 'Project activity feed' : undefined}
		tabindex="0"
		class="stream-body"
	>
		{#if activePhase === 'loading' && !hasPartialEntries}
			<div class="stream-empty" role="status" aria-live="polite">
				<p>{isDecisionTab ? 'Loading decisions…' : 'Loading activity stream…'}</p>
			</div>
		{:else if activePhase === 'error' && !hasPartialEntries}
			<div class="stream-empty" role="alert">
				<AlertTriangle class="size-7" style="color: var(--exec-warn);" aria-hidden="true" />
				<h3>{isDecisionTab ? 'Unable to load decisions' : 'Unable to load activity feed'}</h3>
				<p>{activeError ?? 'The office did not respond in time.'}</p>
				{#if isDecisionTab ? onRetryDecisions : onRetry}
					<button
						type="button"
						onclick={handleRetryClick}
						class="stream-retry-btn min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
					>
						<RefreshCw class="size-3.5" aria-hidden="true" />
						<span>Retry</span>
					</button>
				{/if}
			</div>
		{:else}
			{#if activePhase === 'error' && hasPartialEntries}
				<div class="stream-partial-alert" role="alert">
					<AlertTriangle
						class="size-4 shrink-0"
						style="color: var(--exec-warn);"
						aria-hidden="true"
					/>
					<p>
						Live activity feed unavailable ({activeError ?? 'office did not respond'}) — showing
						loaded project history.
					</p>
					{#if onRetry}
						<button
							type="button"
							onclick={handleRetryClick}
							class="stream-retry-btn min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
						>
							<RefreshCw class="size-3.5" aria-hidden="true" />
							<span>Retry</span>
						</button>
					{/if}
				</div>
			{/if}
			{#if filteredEntries.length === 0}
				<div class="stream-empty">
					<Activity class="size-7" style="color: var(--fg-secondary);" aria-hidden="true" />
					<h3>
						{activeMore > 0
							? 'No matching activity in the latest loaded events'
							: projectScope && activeFilter === 'all'
								? 'No activity yet'
								: 'No matching activity'}
					</h3>
					{#if showEmptyInboxShortcut}
						<p>No matching activity events — {openInboxCount} waiting in Inbox.</p>
						<a
							href={resolve('/inbox')}
							class="stream-inbox-action min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
						>
							<Inbox class="size-3.5" aria-hidden="true" />
							<span>Open Inbox</span>
						</a>
					{:else if showEmptyWorkShortcut}
						<p>
							No matching activity events — {actionableWorkCount}
							{actionableWorkCount === 1 ? 'work item needs' : 'work items need'} attention.
						</p>
						<a
							href={resolve('/work')}
							class="stream-inbox-action min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
						>
							<span>Open Work</span>
						</a>
					{:else}
						<p>
							{#if activeFilter === 'needs_you'}
								{#if inboxPhase === 'loading' || workPhase === 'loading'}
									No matching activity events yet — checking Inbox and Work…
								{:else if inboxPhase === 'error' || workPhase === 'error'}
									No matching activity events — Inbox or Work status unavailable.
								{:else if activeMore > 0}
									No matching activity in the latest loaded events.
								{:else}
									Nothing in this scope is currently waiting on your review.
								{/if}
							{:else if activeMore > 0}
								No matching activity in the latest loaded events.
							{:else}
								Updates across work, runs, conversations, and decisions will appear here.
							{/if}
						</p>
					{/if}
				</div>
			{:else}
				{#each dayGroups as group (group.key)}
					<section class="activity-day" aria-labelledby={`activity-day-${group.key}`}>
						<h2 id={`activity-day-${group.key}`}>{group.label}</h2>
						<ol class="activity-timeline" role="list">
							{#each group.entries as entry (entry.id)}
								<ActivityStreamRow
									{entry}
									when={entryTimestamp(entry.at)}
									isExpanded={expandedRuns.has(entry.id)}
									onToggleRunLog={toggleRunLog}
								/>
							{/each}
						</ol>
					</section>
				{/each}
			{/if}
		{/if}
	</div>
</section>

<style>
	.stream-panel {
		min-width: 0;
		max-width: 100%;
		border: 0;
		border-radius: 0;
		background: transparent;
		-webkit-backdrop-filter: none;
		backdrop-filter: none;
	}
	.stream-tabs-wrap {
		overflow-x: auto;
		border-bottom: 1px solid var(--line);
		background: transparent;
	}
	.stream-tabs {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 0 2px;
	}
	.stream-tab {
		position: relative;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 6px;
		padding: 0 10px;
		border: 0;
		border-bottom: 2px solid transparent;
		background: transparent;
		color: var(--fg-secondary);
		font-size: 0.8125rem;
		font-weight: 550;
		white-space: nowrap;
		cursor: pointer;
	}
	.stream-tab:hover {
		color: var(--exec-ink);
	}
	.stream-tab:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	.stream-tab.is-active {
		color: var(--exec-ink);
		border-bottom-color: var(--tone-accent);
	}
	.stream-tab-badge {
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
	.stream-body:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	.stream-empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 8px;
		padding: 48px 24px;
		text-align: center;
	}
	.stream-empty h3 {
		color: var(--exec-ink);
		font-size: 0.9375rem;
		font-weight: 600;
	}
	.stream-empty p {
		max-width: 24rem;
		color: var(--fg-secondary);
		font-size: 0.8125rem;
	}
	.stream-retry-btn {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		margin-top: 4px;
		padding: 0 14px;
		border: 0;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		color: var(--exec-ink);
		font-size: 0.8125rem;
		font-weight: 600;
		cursor: pointer;
	}
	.stream-retry-btn:hover {
		background: var(--exec-surface-hi);
	}
	.stream-retry-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.stream-inbox-action {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 0 11px;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
		color: var(--exec-warn);
		font-size: 0.75rem;
		font-weight: 600;
		text-decoration: none;
	}
	.stream-inbox-action:hover {
		box-shadow: inset 0 0 0 1.5px var(--exec-warn);
	}
	.stream-inbox-action:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}
	.activity-day h2 {
		margin: 0;
		padding: 14px 8px 2px;
		color: var(--fg-secondary);
		font-size: 0.8125rem;
		font-weight: 600;
	}
	.activity-day + .activity-day {
		margin-block-start: 8px;
	}
	.activity-timeline {
		--activity-entry-padding: 14px;
		--timeline-gutter: 8px;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.stream-partial-alert {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 12px;
		margin-block-end: 14px;
		padding: 10px 12px;
		border: 1px solid color-mix(in srgb, var(--exec-warn) 42%, var(--line));
		border-radius: var(--exec-ctrl-radius, 8px);
		background: color-mix(in srgb, var(--exec-warn) 8%, var(--surface-raised, transparent));
		color: var(--exec-ink);
		font-size: 0.8125rem;
		line-height: 1.4;
	}
	.stream-partial-alert p {
		flex: 1 1 220px;
		margin: 0;
	}
	@media (forced-colors: active) {
		.stream-tab.is-active {
			border: 2px solid Highlight;
		}
		.stream-inbox-action,
		.stream-retry-btn {
			border: 1px solid ButtonText;
		}
	}
</style>
