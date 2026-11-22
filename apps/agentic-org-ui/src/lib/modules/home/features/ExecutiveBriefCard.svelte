<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Check from '@lucide/svelte/icons/check';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clipboard from '@lucide/svelte/icons/clipboard';
	import Clock from '@lucide/svelte/icons/clock';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import type { BriefLens, CatchUpRow, ExecutiveBrief } from '$lib/home/model';

	interface Props {
		brief: ExecutiveBrief;
		catchUp: readonly CatchUpRow[];
	}

	let { brief, catchUp }: Props = $props();

	const BRIEF_LENSES: readonly { id: BriefLens; label: string }[] = [
		{ id: 'today', label: 'Today' },
		{ id: 'week', label: '7-Day' },
		{ id: 'board', label: 'Board' }
	];

	let briefLens = $state<BriefLens>('today');
	let briefExpanded = $state(false);
	let briefCopied = $state(false);
	let briefCopyTimer: ReturnType<typeof setTimeout> | undefined;

	const activeBrief = $derived(brief.lenses[briefLens]);

	onDestroy(() => {
		clearTimeout(briefCopyTimer);
	});

	function selectLens(lens: BriefLens) {
		briefLens = lens;
		briefCopied = false;
	}

	function handleBriefTabKeydown(event: KeyboardEvent, index: number) {
		const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
		if (!step && event.key !== 'Home' && event.key !== 'End') return;
		event.preventDefault();
		const nextIdx =
			event.key === 'Home'
				? 0
				: event.key === 'End'
					? BRIEF_LENSES.length - 1
					: (index + step + BRIEF_LENSES.length) % BRIEF_LENSES.length;
		const nextLens = BRIEF_LENSES[nextIdx].id;
		selectLens(nextLens);
		void tick().then(() => document.getElementById(`exec-brief-tab-${nextLens}`)?.focus());
	}

	function copyBrief() {
		if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
			void navigator.clipboard.writeText(activeBrief.copyText).catch(() => undefined);
		}
		briefCopied = true;
		clearTimeout(briefCopyTimer);
		briefCopyTimer = setTimeout(() => {
			briefCopied = false;
		}, 2400);
		toast.success(`${activeBrief.label} executive brief copied to clipboard.`);
	}
</script>

<aside class="brief-panel flex flex-col gap-3.5" aria-label="Executive Brief">
	<div class="flex flex-wrap items-center justify-between gap-2">
		<h2 class="inline-flex items-center gap-2 text-sm font-semibold text-[var(--exec-ink)]">
			<Sparkles class="size-4 text-[var(--exec-accent-ink)]" aria-hidden="true" />
			<span>Executive Brief</span>
		</h2>
		<div
			class="brief-lenses inline-flex items-center gap-0.5 p-0.5"
			role="tablist"
			aria-label="Brief timeframe"
		>
			{#each BRIEF_LENSES as option, idx (option.id)}
				<button
					type="button"
					role="tab"
					id="exec-brief-tab-{option.id}"
					aria-selected={briefLens === option.id}
					aria-controls="exec-brief-scroll"
					tabindex={briefLens === option.id ? 0 : -1}
					onclick={() => selectLens(option.id)}
					onkeydown={(e) => handleBriefTabKeydown(e, idx)}
					class={[
						'lens-tab inline-flex min-h-9 items-center px-2.5 py-1 text-[11px] font-semibold',
						briefLens === option.id && 'is-on'
					]}
				>
					{option.label}
				</button>
			{/each}
		</div>
	</div>

	<div class={['brief-memo flex min-h-0 flex-1 flex-col gap-3', briefExpanded && 'is-expanded']}>
		<div
			id="exec-brief-scroll"
			role="tabpanel"
			aria-labelledby="exec-brief-tab-{briefLens}"
			class="brief-scroll flex min-h-0 flex-1 flex-col"
			tabindex="0"
		>
			{#each activeBrief.sections as sec (sec.id)}
				<div class="brief-entry flex flex-col gap-1">
					<span class={['brief-kicker', `tone-${sec.tone}`]}>{sec.kicker}</span>
					<p class="text-[13px] leading-relaxed text-[var(--exec-body)]">
						{sec.body}
					</p>
				</div>
			{/each}

			{#if briefLens === 'today' && catchUp.length > 0}
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
								<span class="shrink-0 text-[11px] text-[var(--exec-dim)] tabular-nums">
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
			<span>{briefExpanded ? 'Show less' : 'Show full brief'}</span>
			<ChevronDown
				class={['size-3.5 transition-transform', briefExpanded && 'rotate-180']}
				aria-hidden="true"
			/>
		</button>

		<div class="brief-sig mt-auto flex flex-wrap items-center justify-between gap-2 pt-3">
			<a
				href={resolve('/activity')}
				class="inline-flex items-center gap-1 text-xs font-medium text-[var(--exec-muted)]"
			>
				<span>Full activity log</span>
				<ChevronRight class="size-3.5" aria-hidden="true" />
			</a>
			<div class="flex items-center gap-2">
				<span class="text-[11px] text-[var(--exec-dim)]">
					{activeBrief.updatedLabel}
				</span>
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
		backdrop-filter: var(--exec-panel-blur);
		-webkit-backdrop-filter: var(--exec-panel-blur);
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
			height: 0;
			min-height: 100%;
			overflow: hidden;
		}
	}
	.brief-lenses {
		border-radius: var(--exec-lenses-radius);
		background: var(--exec-well);
		box-shadow: var(--exec-lenses-shadow);
	}
	.lens-tab {
		border-radius: var(--exec-sm-radius);
		color: var(--exec-muted);
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.lens-tab:hover {
		color: var(--exec-ink);
	}
	.lens-tab.is-on {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.brief-memo {
		background: var(--exec-memo-bg);
		box-shadow: var(--exec-memo-shadow);
		border-radius: var(--exec-memo-radius);
		padding: var(--exec-memo-pad);
	}
	.brief-scroll {
		gap: var(--exec-memo-gap);
		overflow-y: auto;
		padding-right: 4px;
		scrollbar-width: thin;
		scrollbar-color: var(--exec-border-hi) transparent;
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
	.brief-entry + .brief-entry {
		border-top: var(--exec-col-divider);
	}
	.brief-kicker {
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.7rem;
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
			padding-right: 0;
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
		.lens-tab,
		.brief-expand,
		.brief-copy {
			min-height: 44px;
		}
	}
</style>
