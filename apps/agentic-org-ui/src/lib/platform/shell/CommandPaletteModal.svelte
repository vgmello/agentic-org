<!--
  @component
  Modal Command Palette dialog (`⌘K` / `/`). Lazy-loaded by `ExecutiveCommandBar.svelte`
  on first open so `Dialog.Root` and palette UI stay out of shell first-paint JS.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import Search from '@lucide/svelte/icons/search';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import * as Dialog from '$lib/components/ui/dialog';
	import { cn } from '$lib/utils';
	import CommandPaletteResults from './CommandPaletteResults.svelte';
	import { buildAskTessEntries, type CommandEntry } from './command-palette-entries';

	let {
		open = $bindable(false),
		modalMode = $bindable<'cmd' | 'ai'>('cmd'),
		query = $bindable(''),
		entries,
		isChannelQuery,
		safeActiveIdx,
		activeOptionId,
		skipRestoreFocus = $bindable(false),
		onResetModalState,
		onResetActive,
		onHashQueryTyped,
		onInputKeydown,
		onSelectIndex,
		onRunEntry,
		onAskTess
	}: {
		open: boolean;
		modalMode: 'cmd' | 'ai';
		query: string;
		entries: readonly CommandEntry[];
		isChannelQuery: boolean;
		safeActiveIdx: number;
		activeOptionId?: string;
		skipRestoreFocus: boolean;
		onResetModalState: () => void;
		onResetActive: () => void;
		onHashQueryTyped: () => void;
		onInputKeydown: (event: KeyboardEvent) => void;
		onSelectIndex: (idx: number) => void;
		onRunEntry: (entry: CommandEntry) => void;
		onAskTess: (prompt: string) => void;
	} = $props();

	let inputEl = $state<HTMLInputElement | null>(null);
	const kbdClass = 'exec-cmdk-foot-kbd';

	export function focusInput(selectEnd = false) {
		void tick().then(() => {
			if (!inputEl) return;
			inputEl.focus();
			if (selectEnd && query.length > 0) {
				inputEl.setSelectionRange(query.length, query.length);
			}
		});
	}
</script>

<Dialog.Root
	bind:open
	onOpenChange={(next) => {
		if (!next) onResetModalState();
	}}
>
	<Dialog.Content
		showCloseButton={false}
		overlayClass="exec-cmdk-overlay"
		onOpenAutoFocus={(event) => {
			event.preventDefault();
			inputEl?.focus();
			if (inputEl && query.length > 0) {
				inputEl.setSelectionRange(query.length, query.length);
			}
		}}
		onCloseAutoFocus={(event) => {
			if (skipRestoreFocus) {
				event.preventDefault();
				skipRestoreFocus = false;
			}
		}}
		class="exec-cmdk-dialog exec-cmdk-surface top-[9vh] flex max-h-[78vh] w-full max-w-[calc(100%-2rem)] translate-y-0 flex-col gap-0 overflow-hidden p-0 ring-0 motion-reduce:animate-none motion-reduce:transition-none motion-reduce:duration-0 motion-reduce:data-closed:animate-none motion-reduce:data-open:animate-none sm:max-w-[650px]"
	>
		<Dialog.Title class="sr-only" text="Command palette" />
		<Dialog.Description class="sr-only">
			Search projects, work, team members, run commands, or ask Tess.
		</Dialog.Description>

		<div class="exec-cmdk-modal-inner flex min-h-0 flex-1 flex-col">
			<div class="exec-cmdk-tabs flex items-center justify-between gap-2.5">
				<div
					class="exec-cmdk-tab-grp inline-flex items-center gap-1"
					role="tablist"
					aria-label="Palette mode"
				>
					<button
						type="button"
						role="tab"
						aria-selected={modalMode === 'cmd'}
						onclick={() => {
							modalMode = 'cmd';
							focusInput();
						}}
						class="exec-cmdk-tab inline-flex items-center gap-1.5"
					>
						<Search class="size-3.5 shrink-0" aria-hidden="true" />
						<span>Search &amp; Commands</span>
					</button>
					<button
						type="button"
						role="tab"
						aria-selected={modalMode === 'ai'}
						onclick={() => {
							modalMode = 'ai';
							focusInput();
						}}
						class="exec-cmdk-tab inline-flex items-center gap-1.5"
					>
						<Sparkles class="size-3.5 shrink-0" aria-hidden="true" />
						<span>Ask Tess</span>
					</button>
				</div>
				<Dialog.Close class={cn('exec-cmdk-close inline-flex items-center justify-center')}>
					ESC
				</Dialog.Close>
			</div>

			<div class="exec-cmdk-input-row flex items-center gap-3">
				{#if modalMode === 'ai'}
					<Sparkles class="size-5 shrink-0 text-[var(--tone-accent-ink)]" aria-hidden="true" />
				{:else}
					<Search class="size-5 shrink-0 text-[var(--tone-accent-ink)]" aria-hidden="true" />
				{/if}
				<input
					bind:this={inputEl}
					bind:value={query}
					oninput={(event) => {
						onResetActive();
						if (modalMode === 'cmd' && event.currentTarget.value.trimStart().startsWith('#')) {
							onHashQueryTyped();
						}
					}}
					onkeydown={onInputKeydown}
					type="text"
					role="combobox"
					aria-label={modalMode === 'ai' ? 'Ask Tess' : 'Search, run a command, or ask Tess'}
					aria-autocomplete="list"
					aria-expanded={modalMode === 'cmd' && entries.length > 0}
					aria-controls={modalMode === 'cmd' && entries.length > 0
						? 'exec-cmdk-listbox'
						: undefined}
					aria-activedescendant={modalMode === 'cmd' ? activeOptionId : undefined}
					placeholder={modalMode === 'ai'
						? "Ask Tess — what's blocked, release pace, budget, or idle agents…"
						: 'Type a command, search anything, or ask a question…'}
					class="exec-cmdk-input min-w-0 flex-1 bg-transparent text-base text-[var(--exec-ink)] outline-none"
				/>
				{#if modalMode === 'ai'}
					<button
						type="button"
						disabled={!query.trim()}
						onclick={() => {
							const q = query.trim();
							if (q) onAskTess(q);
						}}
						class="exec-cmdk-ask-go inline-flex shrink-0 items-center gap-1.5"
					>
						Ask
					</button>
				{/if}
			</div>

			<div class="min-h-0 flex-1 overflow-y-auto p-2.5">
				{#if modalMode === 'ai'}
					<div class="exec-cmdk-ai-pane flex flex-col gap-3 p-1.5">
						<span class="exec-cmdk-grp">Suggested executive questions</span>
						<div class="flex flex-wrap gap-2 px-1">
							{#each buildAskTessEntries(onAskTess) as entry (entry.id)}
								{@const promptText = entry.label.replace(/^Ask:\s*/i, '')}
								<button
									type="button"
									onclick={() => onAskTess(promptText)}
									class="exec-cmdk-ai-chip inline-flex items-center gap-1.5"
								>
									<Sparkles
										class="size-3.5 shrink-0 text-[var(--tone-accent-ink)]"
										aria-hidden="true"
									/>
									<span>{promptText}</span>
								</button>
							{/each}
						</div>
					</div>
				{:else}
					<CommandPaletteResults
						{entries}
						{query}
						{isChannelQuery}
						listboxId="exec-cmdk-listbox"
						optPrefix="exec-cmdk-opt"
						preventBlur={false}
						{safeActiveIdx}
						{onSelectIndex}
						{onRunEntry}
						{onAskTess}
					/>
				{/if}
			</div>

			<div class="exec-cmdk-foot" aria-hidden="true">
				{#if modalMode === 'cmd'}
					<span><kbd class={kbdClass}>↑</kbd><kbd class={kbdClass}>↓</kbd> Navigate</span>
				{/if}
				<span><kbd class={kbdClass}>↵</kbd> Run / Ask</span>
				<span><kbd class={kbdClass}>⌘/</kbd> Switch Search &amp; AI</span>
				<span><kbd class={kbdClass}>Esc</kbd> Close</span>
			</div>
		</div>
	</Dialog.Content>
</Dialog.Root>

<style>
	.exec-cmdk-modal-inner {
		background: var(--exec-cmdk-menu-bg);
		-webkit-backdrop-filter: var(--exec-cmdk-menu-blur);
		backdrop-filter: var(--exec-cmdk-menu-blur);
		box-shadow: var(--exec-cmdk-menu-shadow);
		border-radius: var(--exec-cmdk-dialog-radius, 18px);
		line-height: 1.714;
		letter-spacing: 0.008em;
		font-optical-sizing: none;
	}
	.exec-cmdk-tabs {
		padding: 12px 16px 10px;
		border-bottom: 1px solid var(--exec-border);
		background: var(--exec-cmdk-tabs-bg, var(--exec-well));
	}
	.exec-cmdk-tab-grp {
		padding: 3px;
		border-radius: var(--exec-cmdk-tab-grp-radius, 10px);
		background: var(--exec-cmdk-tab-grp-bg, color-mix(in srgb, var(--exec-ink) 5%, transparent));
		box-shadow: var(--exec-cmdk-tab-grp-shadow, inset 0 0 0 1px var(--exec-border));
	}
	.exec-cmdk-tab {
		padding: 5px 12px;
		border-radius: var(--exec-cmdk-tab-radius, 8px);
		font-size: 0.78125rem;
		line-height: 1.5;
		font-weight: 600;
		color: var(--exec-muted);
		cursor: pointer;
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.exec-cmdk-tab:hover {
		color: var(--exec-ink);
	}
	.exec-cmdk-tab[aria-selected='true'] {
		background: var(--exec-cmdk-tab-active-bg, var(--exec-accent-soft));
		color: var(--exec-cmdk-tab-active-ink, var(--exec-ink));
		box-shadow: var(
			--exec-cmdk-tab-active-shadow,
			inset 0 0 0 1px var(--exec-cmdk-opt-ring, var(--exec-accent-ring))
		);
	}
	.exec-cmdk-tab[aria-selected='true'] :global(svg) {
		color: var(--exec-cmdk-tab-active-ico, var(--tone-accent-ink));
	}
	.exec-cmdk-modal-inner :global(.exec-cmdk-close) {
		padding: 3px 8px;
		border-radius: var(--exec-cmdk-esc-radius, 6px);
		background: var(--exec-cmdk-esc-bg, var(--exec-cmdk-tag-bg, var(--exec-well)));
		box-shadow: var(--exec-cmdk-esc-shadow, inset 0 0 0 1px var(--exec-border-hi));
		color: var(--exec-cmdk-esc-ink, var(--exec-muted));
		font-family: inherit;
		font-size: 0.6875rem;
		line-height: 1.5;
		font-weight: 600;
		cursor: pointer;
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.exec-cmdk-modal-inner :global(.exec-cmdk-close:hover) {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.exec-cmdk-input-row {
		padding: 16px 18px;
		border-bottom: 1px solid var(--exec-border);
	}
	.exec-cmdk-input {
		line-height: 1.45;
		letter-spacing: 0.008em;
	}
	.exec-cmdk-input::placeholder {
		color: var(--exec-cmdk-trigger-ink);
		opacity: 1;
	}
	.exec-cmdk-ask-go {
		height: 32px;
		padding: 0 14px;
		border-radius: 9px;
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-cmdk-opt-ring, var(--exec-accent-ring));
		font-size: 0.8125rem;
		font-weight: 600;
		cursor: pointer;
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.exec-cmdk-ask-go:hover:not(:disabled) {
		background: var(--exec-cta-bg);
		color: var(--exec-on-accent);
	}
	.exec-cmdk-ask-go:disabled {
		opacity: 0.42;
		cursor: default;
	}
	.exec-cmdk-grp {
		padding: 4px 12px 4px;
		font-size: 0.6875rem;
		line-height: 1.714;
		font-weight: 650;
		letter-spacing: var(--exec-cmdk-grp-tracking, 0.08em);
		text-transform: uppercase;
		color: var(--exec-dim);
	}
	.exec-cmdk-ai-chip {
		padding: 7px 13px;
		border-radius: 999px;
		background: var(--exec-cmdk-tag-bg, var(--exec-well));
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		font-size: 0.8125rem;
		font-weight: 500;
		color: var(--exec-body);
		cursor: pointer;
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.exec-cmdk-ai-chip:hover {
		background: var(--exec-accent-soft);
		box-shadow: inset 0 0 0 1px var(--exec-cmdk-opt-ring, var(--exec-accent-ring));
		color: var(--exec-ink);
	}
	.exec-cmdk-foot {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 8px 16px;
		padding: 9px 16px;
		border-top: 1px solid var(--exec-border);
		background: var(--exec-cmdk-foot-bg, var(--exec-well));
		font-size: 0.72rem;
		line-height: 1.714;
		color: var(--exec-dim);
	}
	.exec-cmdk-foot span {
		display: inline-flex;
		align-items: center;
		gap: 5px;
	}
	.exec-cmdk-foot-kbd {
		padding: 1px 5px;
		border-radius: var(--exec-cmdk-kbd-small-radius, 4px);
		background: var(--exec-cmdk-foot-kbd-bg, var(--exec-cmdk-tag-bg, var(--exec-well)));
		box-shadow: inset 0 0 0 1px var(--exec-cmdk-foot-kbd-ring, var(--exec-border-hi));
		font-family: inherit;
		font-size: 0.6875rem;
		line-height: 1.45;
		color: var(--exec-cmdk-foot-kbd-ink, var(--exec-muted));
	}
	@media (max-width: 720px), (pointer: coarse) {
		.exec-cmdk-modal-inner :global(.exec-cmdk-close) {
			height: auto;
			min-height: 44px;
			min-width: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.exec-cmdk-tab,
		.exec-cmdk-modal-inner :global(.exec-cmdk-close),
		.exec-cmdk-ask-go,
		.exec-cmdk-ai-chip {
			transition: none;
		}
	}
</style>
