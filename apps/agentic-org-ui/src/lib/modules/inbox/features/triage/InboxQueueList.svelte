<script lang="ts">
	import { resolve } from '$app/paths';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import Clock from '@lucide/svelte/icons/clock';
	import Inbox from '@lucide/svelte/icons/inbox';
	import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
	import {
		highlightSegments,
		isDeniedOutcome,
		type TriageGroup,
		type TriageTab
	} from './triage-model';

	let {
		groups,
		visibleCount,
		totalOpenCount,
		activeTab,
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
		selectedId?: string;
		busyId?: string;
		searchQuery: string;
		hasActiveFilters: boolean;
		onSelect: (id: string) => void;
		onResetFilters: () => void;
	} = $props();

	const summaryHeading = $derived(activeTab === 'handled' ? 'Recently handled' : 'Priority first');
	const countLabel = $derived(
		visibleCount === 1 ? '1 item in view' : `${visibleCount} items in view`
	);
</script>

{#snippet highlighted(text: string)}
	{#each highlightSegments(text, searchQuery) as seg, i (`${i}-${seg.match}`)}
		{#if seg.match}<mark class="triage-hit">{seg.text}</mark>{:else}{seg.text}{/if}
	{/each}
{/snippet}

<section class="triage-queue flex min-h-0 min-w-0 flex-col" aria-label="Inbox queue">
	<div class="triage-queue-summary flex flex-wrap items-center justify-between gap-2 px-4 py-2.5">
		<span class="triage-queue-kicker text-[11px] font-semibold tracking-wider uppercase">
			{summaryHeading}
		</span>
		<span class="triage-queue-count font-mono text-[11px] tabular-nums">{countLabel}</span>
	</div>

	<div class="triage-queue-scroll flex-1 overflow-y-auto">
		{#if groups.length === 0}
			<div
				class="triage-empty flex flex-col items-center justify-center gap-2.5 px-6 py-14 text-center"
			>
				{#if hasActiveFilters || (activeTab !== 'all' && totalOpenCount > 0)}
					<span class="triage-empty-icon is-filtered flex size-11 items-center justify-center">
						<Inbox class="size-5" aria-hidden="true" />
					</span>
					<h2 class="triage-empty-title text-sm font-semibold">
						{searchQuery.trim() ? 'No matching items' : 'No items in this view'}
					</h2>
					<p class="triage-empty-copy max-w-[260px] text-xs leading-relaxed">
						{#if searchQuery.trim()}
							Nothing matched “{searchQuery.trim()}”. Clear your search or reset filters to see
							waiting items.
						{:else if activeTab === 'handled'}
							Resolved approvals, decisions, and replies will appear here.
						{:else}
							Nothing matches the current filters. Reset filters to inspect waiting items.
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
					<span class="triage-empty-icon is-ok flex size-11 items-center justify-center">
						<CheckCircle2 class="size-5" aria-hidden="true" />
					</span>
					<h2 class="triage-empty-title text-sm font-semibold">All caught up</h2>
					<p class="triage-empty-copy max-w-[260px] text-xs leading-relaxed">
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
				<div class="triage-group">
					<div class="triage-group-head flex items-center justify-between px-4 py-1.5">
						<h2 class="triage-group-title text-[11px] font-semibold tracking-wider uppercase">
							{group.label}
						</h2>
						<span class="triage-group-count font-mono text-[11px] tabular-nums">
							{group.items.length}
						</span>
					</div>
					<ul role="list" aria-label={group.label} class="triage-group-list">
						{#each group.items as item (item.id)}
							{@const selected = item.id === selectedId}
							{@const isBusy = item.id === busyId}
							{@const denied = isDeniedOutcome(item.handled?.outcome)}
							<li>
								<button
									type="button"
									data-triage-row={item.id}
									data-triage-item={item.id}
									aria-current={selected ? 'true' : undefined}
									onclick={() => onSelect(item.id)}
									class={[
										'triage-item flex w-full flex-col gap-1.5 px-4 py-3.5 text-left max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11',
										selected && 'is-selected',
										(isBusy || item.state === 'handled') && 'is-muted'
									]}
								>
									<!-- Row 1: Avatar + Sender + Kind + Relative Age -->
									<div class="flex w-full items-center justify-between gap-2">
										<div class="flex min-w-0 items-center gap-2">
											<span
												class="triage-avatar-sm relative inline-flex size-5 shrink-0 items-center justify-center rounded-full text-[10px] font-bold"
											>
												{item.actorInitials}
											</span>
											<span class="triage-sender truncate text-xs font-semibold">
												{@render highlighted(item.actorName)}
											</span>
											<span class="triage-dot text-[11px]" aria-hidden="true">·</span>
											<span class="triage-kind truncate text-xs">
												{@render highlighted(item.typeLabel)}
											</span>
										</div>
										<span class="triage-age shrink-0 font-mono text-[11px] tabular-nums">
											{item.ageLabel}
										</span>
									</div>

									<!-- Row 2: 2-line clamped subject title with safe substring highlight -->
									<span
										class={[
											'triage-item-title line-clamp-2 pl-7 text-sm leading-snug',
											selected ? 'font-semibold' : 'font-medium',
											isBusy && 'is-busy'
										]}
									>
										{@render highlighted(item.title)}
									</span>

									<!-- Row 3: Project chip + Urgency clock / Handled status badge -->
									<div class="flex flex-wrap items-center gap-2 pl-7">
										<span
											class="triage-chip inline-flex items-center px-2 py-0.5 text-[11px] font-medium"
										>
											{@render highlighted(item.projectName)}
										</span>

										{#if item.state === 'open' && item.expiresLabel}
											<span
												class="triage-urgency inline-flex items-center gap-1 px-2 py-0.5 text-[11px] font-semibold"
											>
												<Clock class="size-3 shrink-0" aria-hidden="true" />
												<span>{item.expiresLabel}</span>
											</span>
										{:else if item.state === 'handled' && item.handled}
											<span
												class={[
													'triage-handled-pill inline-flex items-center px-2 py-0.5 text-[11px] font-semibold',
													denied ? 'is-denied' : 'is-ok'
												]}
											>
												{item.handled.outcome}
											</span>
										{/if}
									</div>
								</button>
							</li>
						{/each}
					</ul>
				</div>
			{/each}
		{/if}
	</div>
</section>

<style>
	.triage-queue {
		border-inline-end: 1px solid var(--exec-border);
		background: var(--exec-well);
	}
	@media (max-width: 959px) {
		.triage-queue {
			border-inline-end: 0;
		}
	}
	.triage-queue-summary {
		border-bottom: 1px solid var(--exec-border);
		color: var(--exec-muted);
	}
	.triage-queue-kicker,
	.triage-queue-count {
		color: var(--exec-muted);
	}
	.triage-queue-scroll {
		overscroll-behavior-y: contain;
	}
	.triage-group-head {
		position: sticky;
		top: 0;
		z-index: 1;
		background: var(--exec-well);
		border-bottom: 1px solid var(--exec-border);
		color: var(--exec-muted);
	}
	.triage-group-title,
	.triage-group-count {
		color: var(--exec-muted);
	}
	.triage-item {
		border-bottom: 1px solid var(--exec-border);
		border-inline-start: 3px solid transparent;
		background: transparent;
		color: var(--exec-body);
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			border-color 120ms ease-out;
	}
	.triage-item:hover {
		background: var(--exec-surface-hi);
	}
	.triage-item:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	.triage-item.is-selected {
		background: var(--exec-accent-soft);
		border-inline-start-color: var(--tone-accent);
	}
	.triage-item.is-selected .triage-sender,
	.triage-item.is-selected .triage-item-title {
		color: var(--exec-ink);
	}
	.triage-avatar-sm {
		background: var(--exec-avatar-bg);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.triage-sender {
		color: var(--exec-ink);
	}
	.triage-dot,
	.triage-kind,
	.triage-age {
		color: var(--exec-muted);
	}
	.triage-item-title {
		color: var(--exec-body);
		overflow-wrap: anywhere;
	}
	.triage-item-title.is-busy {
		color: var(--exec-muted);
		text-decoration: line-through;
	}
	.triage-item:hover .triage-item-title:not(.is-busy) {
		color: var(--exec-ink);
	}
	.triage-chip {
		border-radius: var(--exec-sm-radius);
		background: var(--exec-surface);
		color: var(--exec-muted);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.triage-urgency {
		border-radius: var(--exec-pill-radius);
		background: var(--exec-warn-bg);
		color: var(--exec-warn);
		box-shadow: var(--exec-pill-ring-warn);
	}
	.triage-handled-pill {
		border-radius: var(--exec-pill-radius);
	}
	.triage-handled-pill.is-ok {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		box-shadow: var(--exec-pill-ring-ok);
	}
	.triage-handled-pill.is-denied {
		background: var(--exec-bad-bg);
		color: var(--exec-bad);
		box-shadow: var(--exec-pill-ring-bad);
	}
	.triage-hit {
		background: var(--tone-accent-bg);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
		border-radius: 3px;
		padding: 0 2px;
		font-weight: 600;
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
	}
	.triage-empty-copy {
		color: var(--exec-muted);
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
		.triage-item.is-selected {
			border-inline-start: 4px solid Highlight;
			border-block: 1px solid Highlight;
			outline: 1px solid Highlight;
		}
		.triage-reset-btn,
		.triage-urgency,
		.triage-chip,
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
