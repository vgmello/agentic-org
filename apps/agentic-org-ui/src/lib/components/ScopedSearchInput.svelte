<!--
  @component
  Shared scoped search input for Work views (Team Work, Member Work, Agent Tasks; Prototype Release 1149b4f1).
  - `/` focuses the field (when single-key shortcuts are enabled and no modal overlay is open).
  - `Escape` leaves the typed query intact and moves focus to the page heading.
  - `Cmd/Ctrl+K` continues to propagate to the Workspace command bar.
  - Clear button carries a hover/focus Escape-dismissible tooltip and 44px touch target on narrow/coarse screens.
-->
<script lang="ts">
	import { Tooltip } from 'bits-ui';
	import Search from '@lucide/svelte/icons/search';
	import X from '@lucide/svelte/icons/x';
	import { isEditableTarget, singleKeyShortcutsOn } from '$lib/platform/shell/shortcuts';

	let {
		value = $bindable(''),
		label = 'Search work in this view',
		placeholder = 'Search work…',
		headingSelector = '#team-heading, #project-heading, #shell-page-title, main h1, main h2',
		id,
		class: className = ''
	}: {
		value?: string;
		label?: string;
		placeholder?: string;
		headingSelector?: string;
		id?: string;
		class?: string;
	} = $props();

	const uid = $props.id();
	const inputId = $derived(id ?? `${uid}-scoped-search`);
	let inputEl = $state<HTMLInputElement | null>(null);
	const shortcutsEnabled = $derived(singleKeyShortcutsOn());

	const MODAL_OVERLAY_SELECTOR =
		'[role="dialog"]:not(.work-reading), [role="menu"], [role="alertdialog"], [role="listbox"]';

	function attachSlashShortcut() {
		const onKeydown = (event: KeyboardEvent) => {
			if (event.defaultPrevented || event.isComposing || event.keyCode === 229) return;
			if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey || event.key !== '/') {
				return;
			}
			if (document.querySelector(MODAL_OVERLAY_SELECTOR)) return;
			if (isEditableTarget(event.target)) return;
			if (!inputEl || inputEl.offsetParent === null) return;
			event.stopPropagation();
			if (!shortcutsEnabled || event.repeat) return;
			event.preventDefault();
			inputEl.focus();
		};
		window.addEventListener('keydown', onKeydown, { capture: true });
		return () => {
			window.removeEventListener('keydown', onKeydown, { capture: true });
		};
	}

	function handleKeydown(event: KeyboardEvent) {
		if (event.isComposing || event.keyCode === 229 || event.key !== 'Escape') return;
		// Prototype Release 1149b4f1: Escape leaves the typed query intact and exits the field.
		event.preventDefault();
		event.stopPropagation();
		const heading = document.querySelector<HTMLElement>(headingSelector);
		if (heading) {
			if (!heading.hasAttribute('tabindex')) heading.tabIndex = -1;
			heading.focus({ preventScroll: true });
		} else {
			inputEl?.blur();
		}
	}
</script>

<div class="scoped-search {className}" {@attach attachSlashShortcut}>
	<Search class="scoped-search-ico size-4 shrink-0" aria-hidden="true" />
	<label for={inputId} class="sr-only">{label}</label>
	<input
		bind:this={inputEl}
		id={inputId}
		type="search"
		bind:value
		onkeydown={handleKeydown}
		{placeholder}
		autocomplete="off"
		aria-keyshortcuts={shortcutsEnabled ? '/' : undefined}
		class="scoped-search-input"
		data-testid="scoped-work-search"
	/>
	{#if value}
		<Tooltip.Provider delayDuration={150} ignoreNonKeyboardFocus>
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							type="button"
							aria-label="Clear search"
							onclick={() => {
								value = '';
								inputEl?.focus();
							}}
							class="scoped-clear-btn inline-flex min-h-8 min-w-8 items-center justify-center [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
						>
							<X class="size-3.5" aria-hidden="true" />
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal>
					<Tooltip.Content
						side="bottom"
						sideOffset={6}
						escapeKeydownBehavior="defer-otherwise-close"
						class="z-60 rounded-md border border-line-strong bg-surface-overlay px-2 py-1 text-xs/5 font-medium text-fg shadow-md"
					>
						Clear search
					</Tooltip.Content>
				</Tooltip.Portal>
			</Tooltip.Root>
		</Tooltip.Provider>
	{:else if shortcutsEnabled}
		<kbd class="scoped-search-kbd" aria-hidden="true">/</kbd>
	{/if}
</div>

<style>
	.scoped-search {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		width: min(22rem, 100%);
		min-width: 0;
		min-height: 2.25rem;
		padding: 0 8px 0 10px;
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-well, var(--surface-raised));
	}

	.scoped-search:focus-within {
		border-color: var(--focus);
		outline: 2px solid var(--focus);
		outline-offset: -1px;
	}

	.scoped-search :global(.scoped-search-ico) {
		color: var(--exec-muted, var(--fg-muted));
	}

	.scoped-search-input {
		flex: 1 1 auto;
		min-width: 0;
		width: 100%;
		min-height: 2.125rem;
		border: 0;
		background: transparent;
		color: var(--exec-ink, var(--fg));
		font-size: 0.875rem;
		outline: none;
	}

	.scoped-search-input::placeholder {
		color: var(--exec-muted, var(--fg-muted));
		opacity: 1;
	}

	.scoped-search-input::-webkit-search-cancel-button {
		-webkit-appearance: none;
		appearance: none;
	}

	.scoped-search-kbd {
		flex: none;
		padding: 1px 6px;
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		border-radius: 4px;
		color: var(--exec-muted, var(--fg-muted));
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.75rem;
		line-height: 1.3;
	}

	.scoped-clear-btn {
		border-radius: var(--exec-sm-radius, 6px);
		color: var(--exec-muted, var(--fg-muted));
		cursor: pointer;
	}

	.scoped-clear-btn:hover {
		color: var(--exec-ink, var(--fg));
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
	}

	.scoped-clear-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}

	@media (max-width: 720px), (pointer: coarse) {
		.scoped-search {
			min-height: 2.75rem;
		}
		.scoped-search-kbd {
			display: none;
		}
		.scoped-search-input {
			font-size: 1rem;
		}
	}

	@media (forced-colors: active) {
		.scoped-search {
			border: 1px solid ButtonText;
		}
	}
</style>
