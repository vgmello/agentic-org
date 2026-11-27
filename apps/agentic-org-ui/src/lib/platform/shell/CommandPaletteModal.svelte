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
	import './command-palette.css';

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
				aria-autocomplete="list"
				aria-expanded={modalMode === 'cmd' && entries.length > 0}
				aria-controls={modalMode === 'cmd' && entries.length > 0 ? 'exec-cmdk-listbox' : undefined}
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
					<span class="exec-cmdk-grp is-first">Suggested executive questions</span>
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
	</Dialog.Content>
</Dialog.Root>
