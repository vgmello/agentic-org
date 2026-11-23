<script lang="ts">
	import { resolve } from '$app/paths';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Bell from '@lucide/svelte/icons/bell';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ClipboardList from '@lucide/svelte/icons/clipboard-list';
	import Clock from '@lucide/svelte/icons/clock';
	import Flag from '@lucide/svelte/icons/flag';
	import HelpCircle from '@lucide/svelte/icons/help-circle';
	import Inbox from '@lucide/svelte/icons/inbox';
	import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
	import {
		highlightSegments,
		isDeniedOutcome,
		queueSummaryLabel,
		type TriageGroup,
		type TriageItem,
		type TriageOrder,
		type TriageTab
	} from './triage-model';

	let {
		groups,
		visibleCount,
		totalOpenCount,
		activeTab,
		sortOrder = 'priority',
		selectedId,
		busyId,
		searchQuery,
		hasActiveFilters,
		onSelect,
		onResetFilters
	}: {
		groups: readonly TriageGroup[];
		visibleCount: number;
		totalOpenCount: number;
		activeTab: TriageTab;
		sortOrder?: TriageOrder;
		selectedId?: string;
		busyId?: string;
		searchQuery: string;
		hasActiveFilters: boolean;
		onSelect: (id: string) => void;
		onResetFilters: () => void;
	} = $props();

	const summaryHeading = $derived(queueSummaryLabel(activeTab, sortOrder));
	const countLabel = $derived(
		visibleCount === 1 ? '1 item in view' : `${visibleCount} items in view`
	);

	function rowStateOf(item: TriageItem): 'handled' | 'urgent' | 'pending' {
		if (item.state === 'handled') return 'handled';
		return item.urgent ? 'urgent' : 'pending';
	}

	function rowStateLabelOf(item: TriageItem): string {
		const state = rowStateOf(item);
		if (state === 'handled') return 'Handled';
		if (state === 'urgent') return 'Urgent';
		return item.typeLabel;
	}
</script>

{#snippet highlighted(text: string)}
	{#each highlightSegments(text, searchQuery) as seg, i (`${i}-${seg.match}`)}
		{#if seg.match}<mark class="triage-hit">{seg.text}</mark>{:else}{seg.text}{/if}
	{/each}
{/snippet}

{#snippet categoryIcon(category: TriageItem['category'], isState = false)}
	{#if category === 'approval'}
		{#if isState}
			<Inbox class="size-3.5 shrink-0" aria-hidden="true" />
		{:else}
			<CheckCircle2 class="size-3.5 shrink-0" aria-hidden="true" />
		{/if}
	{:else if category === 'decision'}
		<Flag class="size-3.5 shrink-0" aria-hidden="true" />
	{:else if category === 'question'}
		<HelpCircle class="size-3.5 shrink-0" aria-hidden="true" />
	{:else if category === 'problem'}
		<AlertTriangle class="size-3.5 shrink-0" aria-hidden="true" />
	{:else}
		<ClipboardList class="size-3.5 shrink-0" aria-hidden="true" />
	{/if}
{/snippet}

<section class="triage-queue flex min-h-0 min-w-0 flex-col" aria-label="Inbox queue">
	<div class="triage-queue-summary">
		<span>{summaryHeading}</span>
		<span role="status" class="triage-queue-count tabular-nums">{countLabel}</span>
	</div>

	<div class="triage-queue-scroll flex-1 overflow-y-auto">
		{#if groups.length === 0}
			<div class="triage-empty">
				{#if hasActiveFilters || (activeTab !== 'all' && totalOpenCount > 0)}
					<span class="triage-empty-icon is-filtered flex size-10 items-center justify-center">
						<Inbox class="size-5" aria-hidden="true" />
					</span>
					<h2 class="triage-empty-title">
						{searchQuery.trim() ? 'No matching items' : 'Queue cleared for this view'}
					</h2>
					<p class="triage-empty-copy">
						{#if searchQuery.trim()}
							Nothing matched “{searchQuery.trim()}”. Try another search or reset filters.
						{:else if activeTab === 'handled'}
							Resolved approvals, decisions, and replies will appear here.
						{:else}
							Pick another category or reset filters to see more items.
						{/if}
					</p>
					<button
						type="button"
						onclick={onResetFilters}
						class="triage-reset-btn mt-1.5 inline-flex min-h-9 items-center gap-1.5 px-3.5 text-xs font-semibold max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
					>
						<RotateCcw class="size-3.5" aria-hidden="true" />
						<span>Reset filters</span>
					</button>
				{:else}
					<span class="triage-empty-icon is-ok flex size-10 items-center justify-center">
						<CheckCircle2 class="size-5" aria-hidden="true" />
					</span>
					<h2 class="triage-empty-title">All caught up</h2>
					<p class="triage-empty-copy">
						Nothing is waiting on you right now. Agents and teams will surface new approvals and
						decisions here.
					</p>
					<a
						href={resolve('/home')}
						class="exec-cta mt-2 max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
					>
						<span class="exec-cta-fill" aria-hidden="true"></span>
						<span class="exec-cta-inner"><span>Back to Home</span></span>
					</a>
				{/if}
			</div>
		{:else}
			{#each groups as group (group.key)}
				<section class="triage-group" aria-label={group.label}>
					<div class="triage-group-head">
						<h2>{group.label}</h2>
					</div>
					<ul role="list" aria-label={group.label} class="triage-list">
						{#each group.items as item (item.id)}
							{@const selected = item.id === selectedId}
							{@const isBusy = item.id === busyId}
							{@const denied = isDeniedOutcome(item.handled?.outcome)}
							{@const rowState = rowStateOf(item)}
							{@const stateLabel = rowStateLabelOf(item)}
							<li>
								<button
									type="button"
									data-triage-row={item.id}
									data-triage-item={item.id}
									aria-current={selected ? 'true' : undefined}
									onclick={() => onSelect(item.id)}
									class={[
										'triage-item w-full text-left max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11',
										selected && 'is-selected',
										(isBusy || item.state === 'handled') && 'is-muted'
									]}
								>
									<span class="triage-person">
										<span class="triage-avatar-sm" aria-hidden="true">
											{item.actorInitials}
										</span>
										<span class="triage-sender">
											<strong>{@render highlighted(item.actorName)}</strong>
											<span class="triage-kind">
												<span class="triage-kind-ico" aria-hidden="true">
													{@render categoryIcon(item.category, false)}
												</span>
												<span>{@render highlighted(item.typeLabel)}</span>
											</span>
										</span>
										<span class="triage-stamp">
											<span class="triage-state-icon" data-state={rowState}>
												{#if rowState === 'handled'}
													<CheckCircle2 class="size-3.5 shrink-0" aria-hidden="true" />
												{:else if rowState === 'urgent'}
													<Bell class="size-3.5 shrink-0" aria-hidden="true" />
												{:else}
													{@render categoryIcon(item.category, true)}
												{/if}
												<span class="sr-only">{stateLabel}</span>
											</span>
											<time>{item.ageLabel}</time>
										</span>
									</span>

									<span class={['triage-title', isBusy && 'is-busy']}>
										{@render highlighted(item.title)}
									</span>

									<span class="triage-meta">
										<span>{@render highlighted(item.projectName)}</span>
										{#if item.state === 'handled' && item.handled}
											<span class={['triage-outcome', denied ? 'is-denied' : 'is-ok']}>
												{item.handled.outcome}
											</span>
										{:else if item.expiresLabel}
											<span class="triage-expiry">
												<Clock class="size-3.5 shrink-0" aria-hidden="true" />
												<span>{item.expiresLabel}</span>
											</span>
										{/if}
									</span>
								</button>
							</li>
						{/each}
					</ul>
				</section>
			{/each}
		{/if}
	</div>
</section>

<style>
	.triage-queue {
		--triage-queue-gutter: 16px;
		--triage-row-inset: 8px;
		--triage-row-gap: 4px;
		border-inline-end: 1px solid var(--exec-border);
		background: var(--exec-well);
	}
	@media (max-width: 959px) {
		.triage-queue {
			border-inline-end: 0;
		}
	}
	.triage-queue-summary {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 4px 12px;
		padding: 8px var(--triage-queue-gutter);
		border-bottom: 1px solid var(--exec-border);
		color: var(--exec-muted);
		font-size: 0.75rem;
	}
	.triage-queue-scroll {
		overscroll-behavior-y: contain;
	}
	.triage-group-head {
		padding: 5px var(--triage-queue-gutter);
		background: var(--exec-well);
		color: var(--exec-muted);
	}
	.triage-group-head h2 {
		color: var(--exec-muted);
		font-size: 0.75rem;
		font-weight: 600;
	}
	.triage-list {
		display: grid;
		gap: var(--triage-row-gap);
		padding: var(--triage-row-gap) var(--triage-row-inset);
	}
	.triage-list > li {
		position: relative;
	}
	:global(.triage-workspace[data-inbox-dividers]) .triage-list > li:not(:last-child)::after {
		position: absolute;
		inset: auto 0 calc(var(--triage-row-gap) / -2);
		border-block-end: 1px solid var(--exec-border);
		pointer-events: none;
		content: '';
	}
	:global(.triage-workspace[data-inbox-dividers])
		.triage-list
		> li:not(:last-child):is(:hover, :focus-within)::after {
		border-color: var(--exec-border-hi);
	}
	.triage-item {
		display: grid;
		gap: 4px;
		padding: 12px calc(var(--triage-queue-gutter) - var(--triage-row-inset));
		border-radius: var(--exec-sm-radius);
		background: transparent;
		color: var(--exec-body);
		cursor: pointer;
		transition: background-color 120ms ease-out;
	}
	.triage-item:is(:hover, :focus-visible) {
		background-color: var(--triage-row-hover, var(--exec-surface-hi));
		outline-offset: -2px;
	}
	.triage-item:focus-visible {
		outline: 2px solid var(--focus);
	}
	.triage-item[aria-current='true'] {
		background-color: var(--triage-row-selected, var(--exec-accent-soft));
	}
	.triage-person {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 4px 8px;
		min-width: 0;
	}
	.triage-sender,
	.triage-stamp {
		display: contents;
	}
	.triage-state-icon {
		display: none;
	}
	.triage-avatar-sm {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 24px;
		height: 24px;
		flex: none;
		border-radius: 9999px;
		background: var(--exec-avatar-bg);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		font-size: 0.6875rem;
		font-weight: 700;
	}
	.triage-person strong {
		color: var(--exec-ink);
		font-size: 0.8125rem;
		font-weight: 600;
	}
	.triage-person time {
		margin-inline-start: auto;
		color: var(--exec-muted);
		font-size: 0.75rem;
		font-variant-numeric: tabular-nums;
	}
	.triage-kind {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		color: var(--exec-muted);
		font-size: 0.75rem;
	}
	.triage-kind-ico {
		display: inline-flex;
		align-items: center;
	}
	.triage-title {
		color: var(--exec-ink);
		font-size: 0.875rem;
		font-weight: 600;
		line-height: 1.44;
		padding-inline-start: 32px;
		overflow-wrap: anywhere;
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}
	.triage-title.is-busy {
		color: var(--exec-muted);
		text-decoration: line-through;
	}
	.triage-meta {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 2px 8px;
		padding-inline-start: 32px;
		color: var(--exec-muted);
		font-size: 0.75rem;
	}
	.triage-expiry {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		color: var(--tone-warn-ink);
	}
	.triage-outcome.is-ok {
		color: var(--tone-ok-ink);
	}
	.triage-outcome.is-denied {
		color: var(--tone-bad-ink);
	}
	.triage-hit {
		background: var(--tone-accent-bg);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
		border-radius: 3px;
		padding: 0 2px;
		font-weight: 600;
	}
	.triage-empty {
		display: grid;
		justify-items: start;
		gap: 6px;
		padding: 24px;
		color: var(--exec-muted);
	}
	.triage-empty-icon {
		border-radius: var(--exec-inset-radius);
	}
	.triage-empty-icon.is-filtered {
		background: var(--exec-accent-soft);
		color: var(--exec-accent-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.triage-empty-icon.is-ok {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
	}
	.triage-empty-title {
		color: var(--exec-ink);
		font-size: 0.9375rem;
		font-weight: 600;
	}
	.triage-empty-copy {
		color: var(--exec-muted);
		font-size: 0.8125rem;
		line-height: 1.5;
	}
	.triage-reset-btn {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		cursor: pointer;
		transition: background-color 120ms ease-out;
	}
	.triage-reset-btn:hover {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
	}
	.triage-reset-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}

	@media (forced-colors: active) {
		.triage-item[aria-current='true'] {
			outline: 1px solid Highlight;
			outline-offset: -1px;
		}
		.triage-reset-btn,
		.triage-empty :global(.exec-cta) {
			border: 1px solid ButtonText;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.triage-item,
		.triage-reset-btn {
			transition: none;
		}
	}
</style>
