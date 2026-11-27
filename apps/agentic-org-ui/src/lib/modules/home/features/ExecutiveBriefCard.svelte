<script lang="ts">
	import { onDestroy } from 'svelte';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Check from '@lucide/svelte/icons/check';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import Clipboard from '@lucide/svelte/icons/clipboard';
	import Clock from '@lucide/svelte/icons/clock';
	import FileCheck from '@lucide/svelte/icons/file-check';
	import List from '@lucide/svelte/icons/list';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import type { BriefLens, CatchUpRow, ExecutiveBrief } from '$lib/modules/home/model';

	interface Props {
		brief: ExecutiveBrief;
		catchUp: readonly CatchUpRow[];
		lens?: BriefLens;
		ariaLabelledby?: string;
	}

	let { brief, catchUp, lens = 'today', ariaLabelledby }: Props = $props();

	let briefMode = $state<'highlights' | 'full'>('highlights');
	let briefExpanded = $state(false);
	let copiedLens = $state<BriefLens | null>(null);
	let briefCopyTimer: ReturnType<typeof setTimeout> | undefined;

	const briefCopied = $derived(copiedLens === lens);
	const activeBrief = $derived(brief.lenses[lens]);
	const sigHref = $derived(
		brief.chiefOfStaff ? `/members/${encodeURIComponent(brief.chiefOfStaff.id)}` : '/activity'
	);
	const sigName = $derived(brief.chiefOfStaff?.name ?? 'Tess');
	const sigRole = $derived(brief.chiefOfStaff?.role ?? 'Chief of Staff');

	function firstSentence(text: string): string {
		const trimmed = text.trim();
		const match = trimmed.match(/^.+?[.!?](?:\s|$)/);
		return match ? match[0].trim() : trimmed;
	}

	/** Emphasize the opening factual lead phrase of a brief paragraph without raw HTML injection. */
	function splitLead(text: string): { lead: string; rest: string } | null {
		const markers = [' across ', ' — ', ', ', ' ('];
		let best = -1;
		for (const marker of markers) {
			const idx = text.indexOf(marker);
			if (idx >= 12 && idx <= 44 && (best === -1 || idx < best)) {
				best = idx;
			}
		}
		if (best === -1) return null;
		return { lead: text.slice(0, best), rest: text.slice(best) };
	}

	onDestroy(() => {
		clearTimeout(briefCopyTimer);
	});

	function copyBrief() {
		if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
			void navigator.clipboard.writeText(activeBrief.copyText).catch(() => undefined);
		}
		copiedLens = lens;
		clearTimeout(briefCopyTimer);
		briefCopyTimer = setTimeout(() => {
			copiedLens = null;
		}, 2400);
		toast.success(`${activeBrief.label} executive brief copied to clipboard.`);
	}
</script>

<aside
	class="brief-panel flex flex-col gap-3.5"
	aria-labelledby={ariaLabelledby}
	aria-label={ariaLabelledby ? undefined : 'Executive digest'}
>
	<div class={['brief-memo flex min-h-0 flex-1 flex-col gap-3', briefExpanded && 'is-expanded']}>
		<div
			id="exec-brief-scroll"
			role="tabpanel"
			aria-labelledby="exec-brief-tab-{lens}"
			class="brief-scroll flex min-h-0 flex-1 flex-col"
			tabindex="0"
		>
			{#if briefMode === 'highlights'}
				{#if activeBrief.sections[0]}
					{@const leadSec = activeBrief.sections[0]}
					<p class="brief-lead text-[0.9375rem] leading-[1.58] text-[var(--exec-body)]">
						{firstSentence(leadSec.body)}
					</p>
				{/if}
				<ul class="dg-list flex flex-col" role="list">
					{#each activeBrief.sections as sec (sec.id)}
						<li class={['dg-item grid items-start gap-3', `tone-${sec.tone}`]}>
							<span class="dg-ic mt-0.5 inline-flex shrink-0" aria-hidden="true">
								{#if sec.tone === 'ok'}
									<CheckCircle2 class="size-4" />
								{:else if sec.tone === 'warn'}
									<AlertTriangle class="size-4" />
								{:else}
									<Sparkles class="size-4" />
								{/if}
							</span>
							<div class="flex min-w-0 flex-col gap-0.5">
								<span class="dg-title text-sm font-semibold text-[var(--exec-ink)]">
									{sec.kicker}
								</span>
								<span class="dg-detail text-xs leading-relaxed text-[var(--exec-muted)]">
									{firstSentence(sec.body)}
								</span>
							</div>
						</li>
					{/each}
				</ul>
			{:else}
				{#each activeBrief.sections as sec (sec.id)}
					{@const split = splitLead(sec.body)}
					<div class="brief-entry flex flex-col gap-1.5">
						<span class={['brief-kicker', `tone-${sec.tone}`]}>{sec.kicker}</span>
						<p class="text-[0.9375rem] leading-[1.58] text-[var(--exec-body)]">
							{#if split}
								<strong class="font-semibold text-[var(--exec-ink)]">{split.lead}</strong
								>{split.rest}
							{:else}
								{sec.body}
							{/if}
						</p>
					</div>
				{/each}
			{/if}

			{#if lens === 'today' && catchUp.length > 0}
				<div class="brief-entry is-catchup flex flex-col gap-1.5">
					<span class="brief-kicker tone-accent">Recent Catch-up</span>
					<div class="flex flex-col gap-1.5">
						{#each catchUp as row (row.id)}
							<a
								href={resolve(row.href as '/activity')}
								class="catchup-row flex items-start gap-2 rounded-lg p-2 text-xs"
							>
								<span class="mt-0.5 flex size-3.5 shrink-0 items-center justify-center">
									{#if row.tone === 'ok'}
										<CheckCircle2 class="size-3.5 text-[var(--exec-ok)]" aria-hidden="true" />
									{:else if row.tone === 'warn'}
										<AlertTriangle class="size-3.5 text-[var(--exec-warn)]" aria-hidden="true" />
									{:else if row.tone === 'live'}
										<span class="size-2 rounded-full bg-[var(--exec-ok)]"></span>
									{:else}
										<Clock class="size-3.5 text-[var(--exec-accent-ink)]" aria-hidden="true" />
									{/if}
								</span>
								<span class="min-w-0 flex-1 leading-snug text-[var(--exec-body)]">
									<strong class="text-[var(--exec-ink)]">{row.title}</strong> — {row.detail}
								</span>
								<span class="shrink-0 text-xs text-[var(--exec-dim)] tabular-nums">
									{row.timeLabel}
								</span>
							</a>
						{/each}
					</div>
				</div>
			{/if}
		</div>

		<button
			type="button"
			onclick={() => (briefExpanded = !briefExpanded)}
			aria-expanded={briefExpanded}
			aria-controls="exec-brief-scroll"
			class="brief-expand min-h-9 items-center gap-1.5 self-start px-3 text-xs font-semibold"
		>
			<span>{briefExpanded ? 'Show less' : 'Show more'}</span>
			<ChevronDown
				class={['size-3.5 transition-transform', briefExpanded && 'rotate-180']}
				aria-hidden="true"
			/>
		</button>

		<div class="brief-sig mt-auto flex flex-wrap items-center justify-between gap-2.5 pt-3">
			<a
				href={resolve(sigHref as '/activity')}
				class="sig-who inline-flex min-w-0 items-center gap-2.5"
				title={brief.chiefOfStaff ? `Open ${sigName}'s profile` : 'Open activity log'}
			>
				<span
					class="exec-cos-mark inline-flex size-7 shrink-0 items-center justify-center rounded-[7px]"
					aria-hidden="true"
				>
					<Sparkles class="size-3.5" />
				</span>
				<span class="flex min-w-0 flex-col leading-tight">
					<span class="truncate text-sm font-semibold text-[var(--exec-ink)]">{sigName}</span>
					<span class="truncate text-xs text-[var(--exec-muted)]">{sigRole}</span>
				</span>
			</a>
			<div class="flex flex-wrap items-center justify-end gap-2">
				<div
					class="brief-mode-seg inline-flex items-center gap-0.5 p-0.5"
					role="group"
					aria-label="Brief format"
				>
					<button
						type="button"
						aria-pressed={briefMode === 'highlights'}
						onclick={() => (briefMode = 'highlights')}
						class={[
							'brief-mode-btn inline-flex items-center gap-1 px-2 py-1 text-xs font-semibold',
							briefMode === 'highlights' && 'is-on'
						]}
					>
						<List class="size-3.5" aria-hidden="true" />
						<span>Highlights</span>
					</button>
					<button
						type="button"
						aria-pressed={briefMode === 'full'}
						onclick={() => (briefMode = 'full')}
						class={[
							'brief-mode-btn inline-flex items-center gap-1 px-2 py-1 text-xs font-semibold',
							briefMode === 'full' && 'is-on'
						]}
					>
						<FileCheck class="size-3.5" aria-hidden="true" />
						<span>Full brief</span>
					</button>
				</div>
				<button
					type="button"
					onclick={copyBrief}
					aria-live="polite"
					class={[
						'brief-copy inline-flex min-h-9 items-center gap-1.5 px-2.5 text-xs font-semibold',
						briefCopied && 'is-copied'
					]}
				>
					{#if briefCopied}
						<Check class="size-3.5" aria-hidden="true" />
						<span>Copied</span>
					{:else}
						<Clipboard class="size-3.5" aria-hidden="true" />
						<span>Copy brief</span>
					{/if}
				</button>
			</div>
		</div>
	</div>
</aside>

<style>
	.brief-panel {
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: var(--exec-brief-pad-sm);
		border-top: var(--exec-col-divider);
	}
	.brief-panel:hover {
		background: var(--exec-panel-bg-hover);
		box-shadow: var(--exec-panel-shadow-hover);
	}
	@media (min-width: 1280px) {
		.brief-panel {
			padding: var(--exec-brief-pad);
			border-top: 0;
			border-left: var(--exec-col-divider);
			height: auto;
			min-height: 0;
			overflow: visible;
		}
	}
	.brief-memo {
		background: var(--exec-memo-bg);
		box-shadow: var(--exec-memo-shadow);
		border-radius: var(--exec-memo-radius);
		padding: var(--exec-memo-pad);
	}
	.brief-scroll {
		gap: var(--exec-memo-gap);
	}
	.brief-lead {
		font-weight: 500;
	}
	.dg-list {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	.dg-item {
		grid-template-columns: 18px minmax(0, 1fr);
		padding: 11px 0;
		border-top: 1px solid var(--exec-border);
	}
	.dg-item:last-child {
		padding-bottom: 2px;
	}
	.dg-ic {
		color: var(--exec-muted);
	}
	.dg-item.tone-ok .dg-ic {
		color: var(--exec-ok);
	}
	.dg-item.tone-warn .dg-ic {
		color: var(--exec-warn);
	}
	.dg-item.tone-accent .dg-ic {
		color: var(--exec-accent-ink);
	}
	.brief-entry {
		padding-block: var(--exec-memo-item-py);
	}
	.brief-entry.is-catchup {
		padding-top: max(4px, var(--exec-memo-item-py));
	}
	.brief-entry:first-child {
		padding-top: var(--exec-memo-first-pt);
	}
	.brief-kicker {
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.72rem;
		font-weight: 700;
		letter-spacing: 0.1em;
		text-transform: uppercase;
		color: var(--exec-accent-ink);
	}
	.brief-kicker.tone-ok {
		color: var(--exec-ok);
	}
	.brief-kicker.tone-warn {
		color: var(--exec-warn);
	}
	.brief-mode-seg {
		border-radius: var(--exec-lenses-radius);
		background: var(--exec-well);
		box-shadow: var(--exec-lenses-shadow);
	}
	.brief-mode-btn {
		border-radius: var(--exec-sm-radius);
		color: var(--exec-muted);
		cursor: pointer;
	}
	.brief-mode-btn:hover {
		color: var(--exec-ink);
	}
	.brief-mode-btn.is-on {
		background: var(--exec-lens-on-bg);
		color: var(--exec-lens-on-ink);
		box-shadow: var(--exec-lens-on-shadow);
	}
	.catchup-row:hover {
		background: var(--exec-surface-hi);
	}
	.brief-expand {
		display: none;
		border-radius: var(--exec-sm-radius);
		color: var(--exec-accent-ink);
		background: var(--exec-accent-soft);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.brief-expand:hover {
		background: var(--exec-accent);
		color: var(--exec-on-accent);
	}
	.brief-sig {
		border-top: 1px solid var(--exec-border);
	}
	.exec-cos-mark {
		background: var(--exec-accent-soft);
		color: var(--exec-accent-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.sig-who:hover .exec-cos-mark {
		background: var(--exec-accent);
		color: var(--exec-on-accent);
	}
	.brief-copy {
		border-radius: var(--exec-sm-radius);
		color: var(--exec-accent-ink);
		background: var(--exec-accent-soft);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.brief-copy:hover {
		background: var(--exec-accent);
		color: var(--exec-on-accent);
	}
	.brief-copy.is-copied {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
	}
	@media (max-width: 1279px) {
		.brief-scroll {
			position: relative;
			overflow: hidden;
			max-height: 215px;
		}
		.brief-scroll::after {
			content: '';
			position: absolute;
			left: 0;
			right: 0;
			bottom: 0;
			height: 52px;
			background: linear-gradient(to bottom, transparent, var(--exec-cos-fade));
			pointer-events: none;
		}
		.brief-memo.is-expanded .brief-scroll {
			max-height: none;
			overflow: visible;
		}
		.brief-memo.is-expanded .brief-scroll::after {
			display: none;
		}
		.brief-expand {
			display: inline-flex;
		}
	}
	@media (max-width: 720px), (pointer: coarse) {
		.brief-expand,
		.brief-copy {
			min-height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.brief-copy {
			transition: none;
		}
	}
</style>
