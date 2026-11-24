<script lang="ts">
	import { SvelteSet } from 'svelte/reactivity';
	import { resolve } from '$app/paths';
	import Activity from '@lucide/svelte/icons/activity';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clock from '@lucide/svelte/icons/clock';
	import Inbox from '@lucide/svelte/icons/inbox';
	import MessageSquare from '@lucide/svelte/icons/message-square';
	import Play from '@lucide/svelte/icons/play';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import { relativeAge } from '$lib/home/model';
	import { accountZone, eventTime } from '$lib/modules/preferences';
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
		// Move focus synchronously to the persistent focusable stream container before the Retry button unmounts.
		panel?.focus();
		if (isDecisionTab) onRetryDecisions?.();
		else onRetry?.();
	}

	function handleTabKeydown(event: KeyboardEvent, index: number) {
		let nextIndex: number;
		if (event.key === 'ArrowRight') {
			nextIndex = (index + 1) % tabs.length;
		} else if (event.key === 'ArrowLeft') {
			nextIndex = (index - 1 + tabs.length) % tabs.length;
		} else if (event.key === 'Home') {
			nextIndex = 0;
		} else if (event.key === 'End') {
			nextIndex = tabs.length - 1;
		} else {
			return;
		}

		event.preventDefault();
		const targetTab = tabs[nextIndex];
		activeFilter = targetTab.id;
		const tablist = (event.currentTarget as HTMLElement | null)?.closest('[role="tablist"]');
		tablist?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[nextIndex]?.focus();
	}
</script>

<section class="stream-panel activity-feed" aria-label="Chronological activity stream">
	{#if showTabs}
		<!-- WAI-ARIA Tabs Pattern with roving tabindex and Arrow/Home/End navigation -->
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

	<!-- Compact Bottleneck Distribution Bar (rendered only when work data is ready) -->
	{#if workReady}
		<div class="stream-bottleneck">
			<div class="stream-bottleneck-main">
				<span class="stream-bottleneck-label">Open mix</span>
				<div class="stream-stage-bar" role="img" aria-label={bottleneck.legend}>
					{#if bottleneck.agentsPct > 0}
						<span
							class="seg-agents"
							style:width={`${bottleneck.agentsPct}%`}
							title={`${bottleneck.agentsPct}% agents`}
						></span>
					{/if}
					{#if bottleneck.ciPct > 0}
						<span
							class="seg-ci"
							style:width={`${bottleneck.ciPct}%`}
							title={`${bottleneck.ciPct}% CI / ready for checks`}
						></span>
					{/if}
					{#if bottleneck.reviewPct > 0}
						<span
							class="seg-review"
							style:width={`${bottleneck.reviewPct}%`}
							title={`${bottleneck.reviewPct}% waiting on you`}
						></span>
					{/if}
					{#if bottleneck.queuePct > 0}
						<span
							class="seg-queue"
							style:width={`${bottleneck.queuePct}%`}
							title={`${bottleneck.queuePct}% queued`}
						></span>
					{/if}
				</div>
				<span class="stream-bottleneck-legend tabular-nums">{bottleneck.legend}</span>
			</div>
			<span class="stream-bottleneck-note">{bottleneck.note}</span>
		</div>
	{/if}

	<!-- Inactive hidden tabpanels ensure every tab's aria-controls resolves to a live DOM element in portfolio tablist mode -->
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
		{#if activePhase === 'loading'}
			<div class="stream-empty" role="status" aria-live="polite">
				<p>{isDecisionTab ? 'Loading decisions…' : 'Loading activity stream…'}</p>
			</div>
		{:else if activePhase === 'error'}
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
		{:else if filteredEntries.length === 0}
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
					<p>
						No matching activity events — {openInboxCount} waiting in Inbox.
					</p>
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
							{@const hasLog = Boolean(entry.run?.log && entry.run.log.length > 0)}
							{@const isExpanded = expandedRuns.has(entry.id)}
							{@const when = entryTimestamp(entry.at)}
							<li class="stream-row">
								<div class="stream-row-avatar" data-tone={entry.tone} aria-hidden="true">
									<span>{entry.actorInitials}</span>
								</div>

								<div class="stream-row-content">
									<div class="stream-row-head">
										<span class="stream-actor">{entry.actorName}</span>
										<span class="stream-kind">
											{#if entry.tone === 'ok'}
												<CheckCircle2
													class="size-3.5"
													style="color: var(--exec-ok);"
													aria-hidden="true"
												/>
											{:else if entry.tone === 'bad'}
												<AlertTriangle
													class="size-3.5"
													style="color: var(--exec-bad);"
													aria-hidden="true"
												/>
											{:else if entry.tone === 'warn'}
												<AlertTriangle
													class="size-3.5"
													style="color: var(--exec-warn);"
													aria-hidden="true"
												/>
											{:else if entry.kind === 'conversation'}
												<MessageSquare
													class="size-3.5"
													style="color: var(--exec-sky);"
													aria-hidden="true"
												/>
											{:else if entry.kind === 'run'}
												<Play
													class="size-3.5"
													style="color: var(--tone-accent-ink);"
													aria-hidden="true"
												/>
											{:else}
												<Clock
													class="size-3.5"
													style="color: var(--fg-secondary);"
													aria-hidden="true"
												/>
											{/if}
											<span>{entry.kindLabel}</span>
										</span>
										{#if entry.needsYou}
											<span class="stream-needs-pill">Needs you</span>
										{/if}
										{#each entry.projects as proj (proj)}
											<span class="stream-project-pill">{proj}</span>
										{/each}
									</div>

									<div class="stream-row-text">
										<a
											href={resolve(entry.href as '/work')}
											class="stream-row-link min-h-9 min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
										>
											{entry.text}
										</a>
									</div>

									{#if entry.preview}
										<p class="stream-row-preview">{entry.preview}</p>
									{/if}

									{#if entry.run}
										<p class="stream-run-meta">
											<span>{entry.run.trigger}</span>
											{#if entry.run.outcome}
												<span aria-hidden="true">·</span>
												<span class={entry.run.state === 'failed' ? 'is-bad' : 'is-ok'}>
													{entry.run.outcome}
												</span>
											{/if}
										</p>
									{/if}

									{#if entry.inboxHref}
										<div class="activity-triage">
											{#if entry.inboxTitle}
												<span class="activity-triage-title">{entry.inboxTitle}</span>
											{/if}
											<a
												href={resolve(entry.inboxHref as '/inbox')}
												class="stream-inbox-action min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
											>
												<Inbox class="size-3.5" aria-hidden="true" />
												<span>{entry.inboxLabel ?? 'Review in Inbox'}</span>
											</a>
										</div>
									{/if}

									{#if hasLog}
										<div class="stream-row-actions">
											<button
												type="button"
												aria-expanded={isExpanded}
												aria-controls={`run-log-${entry.id}`}
												onclick={() => toggleRunLog(entry.id)}
												class="stream-log-btn min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
											>
												{#if isExpanded}
													<ChevronDown class="size-3.5" aria-hidden="true" />
													<span>Hide run log</span>
												{:else}
													<ChevronRight class="size-3.5" aria-hidden="true" />
													<span>Show run log</span>
												{/if}
											</button>
										</div>
									{/if}

									{#if hasLog && entry.run?.log}
										<ol
											id={`run-log-${entry.id}`}
											class="stream-run-log runlog"
											hidden={!isExpanded}
										>
											{#each entry.run.log as line, idx (`${entry.id}-log-${idx}`)}
												<li>{line}</li>
											{/each}
										</ol>
									{/if}
								</div>

								<time datetime={when.iso} title={when.full} class="stream-row-time tabular-nums">
									{when.text}
								</time>
							</li>
						{/each}
					</ol>
				</section>
			{/each}
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

	.stream-bottleneck {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 8px 16px;
		padding: 8px var(--activity-gutter, 16px);
		border-bottom: 1px solid var(--line);
		background: var(--workspace-secondary-surface, var(--exec-well));
		font-size: 0.75rem;
	}

	.stream-bottleneck-main {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 10px;
		flex: 1 1 260px;
		min-width: 0;
	}

	.stream-bottleneck-label {
		color: var(--exec-body);
		font-weight: 600;
		flex-shrink: 0;
	}

	.stream-stage-bar {
		display: flex;
		gap: 1px;
		min-width: 92px;
		max-width: 180px;
		flex: 1;
		height: 6px;
		overflow: hidden;
		border-radius: var(--exec-bar-radius, 999px);
		background: var(--bar-track);
	}

	.stream-stage-bar .seg-agents {
		background: var(--exec-seg-agents);
	}
	.stream-stage-bar .seg-ci {
		background: var(--exec-seg-ci);
	}
	.stream-stage-bar .seg-review {
		background: var(--exec-warn);
	}
	.stream-stage-bar .seg-queue {
		background: var(--exec-seg-queued);
	}

	.stream-bottleneck-legend,
	.stream-bottleneck-note {
		color: var(--fg-secondary);
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

	.activity-timeline > li.stream-row {
		position: relative;
		display: flex;
		align-items: flex-start;
		gap: 12px;
		padding: var(--activity-entry-padding) var(--timeline-gutter);
		border-radius: var(--exec-sm-radius, 6px);
		border-bottom: 0;
	}

	.activity-timeline > li.stream-row:not(:last-child)::before {
		content: '';
		position: absolute;
		inset-block: calc(var(--activity-entry-padding) + 34px)
			calc(2px - var(--activity-entry-padding));
		inset-inline-start: calc(var(--timeline-gutter) + 13.5px);
		border-inline-start: 1px solid var(--line-strong);
		pointer-events: none;
	}

	.stream-row:hover {
		background: var(--surface-raised);
	}

	.stream-row-avatar {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 28px;
		height: 28px;
		border-radius: 999px;
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		color: var(--exec-ink);
		font-size: 0.6875rem;
		font-weight: 650;
		flex-shrink: 0;
		margin-top: 2px;
	}

	.stream-row-avatar[data-tone='warn'] {
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
		color: var(--exec-warn);
	}

	.stream-row-avatar[data-tone='bad'] {
		background: var(--exec-bad-bg);
		box-shadow: inset 0 0 0 1px var(--exec-bad-ring);
		color: var(--exec-bad);
	}

	.stream-row-avatar[data-tone='ok'] {
		background: var(--exec-ok-bg);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
		color: var(--exec-ok);
	}

	.stream-row-content {
		flex: 1;
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.stream-row-head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
		font-size: 0.75rem;
		min-width: 0;
	}

	.stream-actor {
		color: var(--exec-ink);
		font-weight: 650;
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.stream-kind {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		color: var(--fg-secondary);
	}

	.stream-needs-pill {
		padding: 1px 7px;
		border-radius: var(--exec-pill-radius, 999px);
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
		color: var(--exec-warn);
		font-size: 0.6875rem;
		font-weight: 650;
	}

	.stream-project-pill {
		padding: 1px 7px;
		border-radius: var(--exec-sm-radius, 6px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		color: var(--fg-secondary);
		font-size: 0.6875rem;
		max-width: 100%;
		overflow-wrap: anywhere;
	}

	.stream-row-text {
		margin-top: 2px;
		font-size: 0.875rem;
		line-height: 1.45;
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.stream-row-link {
		display: inline-flex;
		align-items: center;
		max-width: 100%;
		color: var(--exec-ink);
		font-weight: 550;
		text-decoration: none;
		overflow-wrap: anywhere;
	}

	.stream-row-link:hover {
		color: var(--tone-accent-ink);
		text-decoration: underline;
	}

	.stream-row-link:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}

	.stream-row-preview {
		margin: 2px 0 0;
		color: var(--exec-body);
		font-size: 0.8125rem;
		line-height: 1.45;
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.stream-run-meta {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
		margin: 4px 0 0;
		color: var(--fg-secondary);
		font-size: 0.75rem;
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.stream-run-meta .is-ok {
		color: var(--exec-ok);
		font-weight: 550;
	}

	.stream-run-meta .is-bad {
		color: var(--exec-bad);
		font-weight: 550;
	}

	.activity-triage {
		display: grid;
		justify-items: start;
		gap: 6px;
		width: min(100%, 560px);
		margin-top: 8px;
		padding: 8px 10px;
		border: 1px solid var(--line-strong);
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--surface-raised);
	}

	.activity-triage:focus-within {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}

	.activity-triage-title {
		color: var(--exec-body);
		font-size: 0.8125rem;
		font-weight: 550;
		line-height: 1.4;
		overflow-wrap: anywhere;
	}

	.stream-row-actions {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
		margin-top: 8px;
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

	.stream-inbox-action:focus-visible,
	.stream-log-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}

	.stream-log-btn {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		padding: 0 10px;
		border: 0;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		color: var(--exec-body);
		font-size: 0.75rem;
		font-weight: 550;
		cursor: pointer;
	}

	.stream-log-btn:hover {
		color: var(--exec-ink);
	}

	.stream-run-log {
		margin: 8px 0 0;
		padding: 10px 12px;
		list-style: none;
		overflow-x: auto;
		border: 1px solid var(--line);
		border-radius: var(--exec-sm-radius, 6px);
		background: var(--exec-well);
		color: var(--exec-body);
		font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
		font-size: 0.75rem;
		line-height: 1.5;
		white-space: pre-wrap;
		word-break: break-word;
	}

	.stream-run-log[hidden] {
		display: none;
	}

	.stream-row-time {
		color: var(--fg-secondary);
		font-size: 0.75rem;
		white-space: nowrap;
		flex-shrink: 0;
		padding-block-start: 2px;
	}

	@media (forced-colors: active) {
		.stream-tab.is-active {
			border: 2px solid Highlight;
		}
		.activity-triage,
		.stream-inbox-action,
		.stream-log-btn,
		.stream-retry-btn {
			border: 1px solid ButtonText;
		}
	}
</style>
