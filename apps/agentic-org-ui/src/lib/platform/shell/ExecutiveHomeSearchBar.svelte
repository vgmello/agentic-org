<!--
  @component
  Home inline command bar & notifications header (`ExecutiveCommandBar` child).
  Owns all scoped topbar & inline results popover styling.
-->
<script lang="ts">
	import Search from '@lucide/svelte/icons/search';
	import NotificationsButton from '$lib/components/shell/NotificationsButton.svelte';
	import type CommandPaletteResultsType from './CommandPaletteResults.svelte';
	import type { CommandEntry } from './command-palette-entries';

	let {
		query = $bindable(''),
		homeInputEl = $bindable<HTMLInputElement | null>(null),
		homeShellEl = $bindable<HTMLDivElement | null>(null),
		inlineOpen,
		inlineListboxVisible,
		inlineActiveOptionId,
		isChannelQuery,
		entries,
		safeActiveIdx,
		offline = false,
		PaletteResults,
		onFocusSearch,
		onInputSearch,
		onInputKeydown,
		onSelectIndex,
		onRunEntry,
		onAskTess
	}: {
		query: string;
		homeInputEl: HTMLInputElement | null;
		homeShellEl: HTMLDivElement | null;
		inlineOpen: boolean;
		inlineListboxVisible: boolean;
		inlineActiveOptionId?: string;
		isChannelQuery: boolean;
		entries: readonly CommandEntry[];
		safeActiveIdx: number;
		offline?: boolean;
		PaletteResults?: typeof CommandPaletteResultsType;
		onFocusSearch: () => void;
		onInputSearch: (value: string) => void;
		onInputKeydown: (event: KeyboardEvent) => void;
		onSelectIndex: (idx: number) => void;
		onRunEntry: (entry: CommandEntry) => void;
		onAskTess: (prompt: string) => void;
	} = $props();

	const kbdClass = 'exec-cmdk-foot-kbd';
</script>

<header class="exec-topbar exec-home-topbar relative z-30 shrink-0 overflow-visible px-4 sm:px-6">
	<div
		bind:this={homeShellEl}
		class="exec-home-cmdk relative flex w-full max-w-[640px] min-w-0 flex-col"
	>
		<div class="exec-cmdk-bar flex w-full items-center">
			<label
				class="exec-cmdk-trigger exec-cmdk-home-field flex min-w-0 flex-1 items-center text-left"
			>
				<Search class="exec-cmdk-ico size-[18px] shrink-0" aria-hidden="true" />
				<input
					bind:this={homeInputEl}
					bind:value={query}
					type="search"
					autocomplete="off"
					placeholder="Search, run a command, or ask Tess…"
					aria-label="Search, run a command, or ask Tess"
					role="combobox"
					aria-autocomplete="list"
					aria-expanded={inlineListboxVisible}
					aria-controls={inlineListboxVisible ? 'exec-cmdk-home-listbox' : undefined}
					aria-activedescendant={inlineActiveOptionId}
					onfocus={onFocusSearch}
					oninput={(event) => onInputSearch(event.currentTarget.value)}
					onkeydown={onInputKeydown}
					class="exec-cmdk-home-input min-w-0 flex-1 bg-transparent text-sm text-[var(--exec-ink)] outline-none"
				/>
				<kbd class="exec-cmdk-kbd hidden sm:inline-flex" aria-hidden="true">⌘K</kbd>
			</label>
		</div>

		{#if inlineOpen}
			<div class="exec-cmdk-home-results exec-cmdk-surface flex flex-col overflow-hidden">
				<div class="min-h-0 flex-1 overflow-y-auto p-2.5">
					{#if PaletteResults}
						{@const ResultsComp = PaletteResults}
						<ResultsComp
							{entries}
							{query}
							{isChannelQuery}
							listboxId="exec-cmdk-home-listbox"
							optPrefix="exec-cmdk-home-opt"
							preventBlur={true}
							{safeActiveIdx}
							{onSelectIndex}
							{onRunEntry}
							{onAskTess}
						/>
					{/if}
				</div>
				<div class="exec-cmdk-foot" aria-hidden="true">
					<span><kbd class={kbdClass}>↑</kbd><kbd class={kbdClass}>↓</kbd> Navigate</span>
					<span><kbd class={kbdClass}>↵</kbd> Run / Ask</span>
					<span><kbd class={kbdClass}>⌘/</kbd> Ask Tess</span>
					<span><kbd class={kbdClass}>Esc</kbd> Close</span>
				</div>
			</div>
		{/if}
	</div>
	<div class="exec-home-utils">
		<NotificationsButton {offline} />
	</div>
</header>

<style>
	.exec-topbar {
		height: var(--exec-topbar-height);
		background: var(--exec-topbar-bg);
		border-bottom: var(--exec-topbar-border);
		overflow: visible;
	}
	.exec-home-topbar {
		display: grid;
		grid-template-columns: 1fr minmax(0, 640px) 1fr;
		align-items: center;
		gap: 12px;
	}
	.exec-home-cmdk {
		grid-column: 2;
		justify-self: center;
	}
	.exec-home-utils {
		grid-column: 3;
		justify-self: end;
		display: inline-flex;
		align-items: center;
		gap: 6px;
	}
	@media (max-width: 959px) {
		.exec-home-topbar {
			display: flex;
			justify-content: space-between;
			gap: 10px;
		}
		.exec-home-cmdk {
			flex: 1 1 auto;
		}
	}
	:global(html[data-ui-theme='ocean']) .exec-topbar::before {
		bottom: -14px;
	}
	.exec-cmdk-bar {
		height: 46px;
		padding: 5px 6px;
		border-radius: var(--exec-cmdk-radius);
		background-color: var(--exec-cmdk-bg);
		-webkit-backdrop-filter: var(--exec-cmdk-blur);
		backdrop-filter: var(--exec-cmdk-blur);
		box-shadow: var(--exec-cmdk-shadow);
		transition:
			background-color 0.18s ease,
			box-shadow 0.18s ease,
			transform 0.18s ease;
	}
	.exec-cmdk-home-input::placeholder {
		color: var(--exec-cmdk-trigger-ink);
		opacity: 1;
	}
	.exec-cmdk-bar:hover {
		background-color: var(--exec-cmdk-bg-hover);
		box-shadow: var(--exec-cmdk-shadow-hover);
		transform: var(--exec-cmdk-hover-lift);
	}
	.exec-cmdk-trigger {
		height: 100%;
		gap: 11px;
		padding: 0 10px 0 12px;
		border-radius: var(--exec-cmdk-trigger-radius);
		color: var(--exec-cmdk-trigger-ink);
		font-size: 0.875rem;
		line-height: 1.2;
		font-weight: 450;
		letter-spacing: -0.005em;
		transition:
			color 0.14s ease,
			background-color 0.14s ease;
	}
	.exec-cmdk-home-field {
		cursor: text;
	}
	.exec-cmdk-home-field:focus-within {
		outline: 1px solid var(--exec-border-hi);
		outline-offset: -1px;
	}
	.exec-cmdk-home-results {
		position: absolute;
		top: calc(100% + 8px);
		inset-inline: 0;
		z-index: 40;
		max-height: min(27rem, 58dvh);
		border-radius: var(--exec-cmdk-dialog-radius, 14px);
	}
	.exec-cmdk-surface {
		background: var(--exec-cmdk-menu-bg);
		-webkit-backdrop-filter: var(--exec-cmdk-menu-blur);
		backdrop-filter: var(--exec-cmdk-menu-blur);
		box-shadow: var(--exec-cmdk-menu-shadow);
		line-height: 1.714;
		letter-spacing: 0.008em;
		font-optical-sizing: none;
	}
	.exec-cmdk-trigger:hover {
		color: var(--exec-ink);
		background: var(--exec-cmdk-trigger-hover-bg);
	}
	.exec-cmdk-trigger :global(.exec-cmdk-ico) {
		color: var(--exec-cmdk-icon);
		transition: color 0.14s ease;
	}
	.exec-cmdk-trigger:hover :global(.exec-cmdk-ico) {
		color: var(--exec-cmdk-icon-hover);
	}
	.exec-cmdk-kbd {
		align-items: center;
		justify-content: center;
		height: 24px;
		padding: 0 8px;
		border-radius: var(--exec-cmdk-kbd-radius);
		background: var(--exec-cmdk-kbd-bg);
		box-shadow: var(--exec-cmdk-kbd-shadow);
		color: var(--exec-cmdk-kbd-ink);
		font-family: inherit;
		font-size: 0.72rem;
		font-weight: 600;
		flex: none;
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
		.exec-topbar {
			height: auto;
			min-height: max(68px, var(--exec-topbar-height));
			padding-block: 7px;
		}
		.exec-cmdk-bar {
			height: auto;
			min-height: 52px;
			padding: 4px 6px;
		}
		.exec-cmdk-trigger {
			height: auto;
			min-height: 44px;
		}
	}
	@media (height <= 320px) {
		.exec-topbar {
			height: auto;
			min-height: 44px;
			padding-block: 0;
			border-bottom: 0;
		}
		.exec-cmdk-bar {
			height: auto;
			min-height: 44px;
			padding: 0 4px;
		}
		.exec-cmdk-trigger {
			height: auto;
			min-height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.exec-cmdk-bar,
		.exec-cmdk-trigger,
		.exec-cmdk-trigger :global(.exec-cmdk-ico) {
			transition: none;
		}
	}
</style>
