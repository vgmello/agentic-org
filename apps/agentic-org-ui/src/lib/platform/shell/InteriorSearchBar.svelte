<!--
  @component
  Unified interior page header search + Workspace Cmd+K command bar (matches POC #page-search-tools .cmdk-shell).
  - On pages with local search (explicit props or active scopedSearch context):
    [ <local search input>  [/ or Clear]  |  Workspace Cmd+K ]
  - On pages without local search (Projects, Activity, Teams, Settings, etc.):
    [ Search...  [/]  |  Workspace Cmd+K ]
-->
<script lang="ts">
	import { Tooltip } from 'bits-ui';
	import Search from '@lucide/svelte/icons/search';
	import X from '@lucide/svelte/icons/x';
	import NotificationsButton from './NotificationsButton.svelte';
	import { useScopedSearchContext } from './scoped-search.svelte';
	import { useShellCommand } from './shell-command';
	import { isEditableTarget, singleKeyShortcutsOn } from './shortcuts';

	let {
		value,
		onValueChange,
		onEscape,
		label,
		placeholder,
		id,
		inputTestId,
		headingSelector = '#team-heading, #project-heading, #shell-page-title, main h1, main h2',
		captureSlash = false,
		inputEl = $bindable<HTMLInputElement | null>(null),
		class: className = ''
	}: {
		value?: string;
		onValueChange?: (next: string) => void;
		onEscape?: () => void;
		label?: string;
		placeholder?: string;
		id?: string;
		inputTestId?: string;
		headingSelector?: string;
		captureSlash?: boolean;
		inputEl?: HTMLInputElement | null;
		class?: string;
	} = $props();

	const shellCommand = useShellCommand();
	const scopedSearch = useScopedSearchContext();
	const activeScoped = $derived(onValueChange ? undefined : scopedSearch?.active);
	const uid = $props.id();
	const shortcutsEnabled = $derived(singleKeyShortcutsOn());
	const isMac = $derived(
		typeof navigator !== 'undefined' && /Mac|iPhone|iPad|iPod/.test(navigator.platform)
	);
	const cmdShortcutLabel = $derived(isMac ? '⌘K' : 'Ctrl K');

	let workspaceDraft = $state('');
	let clearTipOpen = $state(false);
	const clearTipTether = Tooltip.createTether();

	const hasLocalMode = $derived(Boolean(onValueChange || activeScoped));
	const effectiveValue = $derived(
		onValueChange ? (value ?? '') : activeScoped ? activeScoped.getValue() : workspaceDraft
	);
	const effectiveLabel = $derived(label ?? activeScoped?.label ?? 'Search across the workspace');
	const effectivePlaceholder = $derived(placeholder ?? activeScoped?.placeholder ?? 'Search…');
	const effectiveId = $derived(id ?? `${uid}-interior-search`);
	const effectiveInputTestId = $derived(
		inputTestId ?? (activeScoped ? 'scoped-work-search' : undefined)
	);

	const MODAL_OVERLAY_SELECTOR =
		'[role="dialog"]:not(.work-reading), [role="menu"], [role="alertdialog"], [role="listbox"]';

	function handleInput(next: string) {
		if (onValueChange) {
			onValueChange(next);
		} else if (activeScoped) {
			activeScoped.setValue(next);
		} else {
			workspaceDraft = next;
		}
	}

	function isBareSlashKey(event: KeyboardEvent): boolean {
		if (event.defaultPrevented || event.isComposing || event.keyCode === 229) return false;
		if (event.key !== '/') return false;
		return !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
	}

	function canFocusSlashInput(target: EventTarget | null): boolean {
		if (document.querySelector(MODAL_OVERLAY_SELECTOR) || isEditableTarget(target)) return false;
		return Boolean(inputEl && inputEl.offsetParent !== null && !inputEl.closest('[inert]'));
	}

	function attachSlashShortcut() {
		const onKeydown = (event: KeyboardEvent) => {
			if (!captureSlash && !activeScoped) return;
			if (!isBareSlashKey(event) || !canFocusSlashInput(event.target)) return;
			event.stopPropagation();
			if (!shortcutsEnabled || event.repeat) return;
			event.preventDefault();
			inputEl?.focus();
		};
		window.addEventListener('keydown', onKeydown, { capture: true });
		return () => {
			window.removeEventListener('keydown', onKeydown, { capture: true });
		};
	}

	function focusExitTarget() {
		const targetSelector = activeScoped?.headingSelector ?? headingSelector;
		const heading = document.querySelector<HTMLElement>(targetSelector);
		if (!heading) {
			inputEl?.blur();
			return;
		}
		if (!heading.hasAttribute('tabindex')) heading.tabIndex = -1;
		heading.focus({ preventScroll: true });
	}

	function handleKeydown(event: KeyboardEvent) {
		if (event.isComposing || event.keyCode === 229) return;
		if (event.key === 'Enter' && !hasLocalMode) {
			event.preventDefault();
			const q = workspaceDraft.trim();
			workspaceDraft = '';
			shellCommand?.openPalette(q);
			return;
		}
		if (event.key !== 'Escape') return;
		clearTipTether.close();
		clearTipOpen = false;
		event.preventDefault();
		event.stopPropagation();
		if (onEscape) onEscape();
		else focusExitTarget();
	}

	function openWorkspacePalette() {
		const prefill = effectiveValue.trim();
		if (!hasLocalMode) workspaceDraft = '';
		shellCommand?.openPalette(prefill);
	}
</script>

<div
	class={['interior-search-tools', className]}
	data-testid={activeScoped ? 'header-scoped-search' : 'interior-search-tools'}
	{@attach attachSlashShortcut}
>
	<div class="interior-cmdk-shell">
		<div class="interior-local-field">
			<Search class="interior-search-ico size-4 shrink-0" aria-hidden="true" />
			<label class="sr-only" for={effectiveId}>{effectiveLabel}</label>
			<input
				bind:this={inputEl}
				id={effectiveId}
				type="search"
				value={effectiveValue}
				oninput={(e) => handleInput(e.currentTarget.value)}
				onkeydown={handleKeydown}
				placeholder={effectivePlaceholder}
				autocomplete="off"
				aria-keyshortcuts={shortcutsEnabled ? '/' : undefined}
				data-testid={effectiveInputTestId}
				class="interior-search-input"
			/>
			{#if effectiveValue}
				<Tooltip.Provider delayDuration={150} ignoreNonKeyboardFocus>
					<Tooltip.Root bind:open={clearTipOpen} tether={clearTipTether}>
						<Tooltip.Trigger>
							{#snippet child({ props })}
								<button
									{...props}
									type="button"
									aria-label="Clear search"
									onclick={() => {
										clearTipTether.close();
										clearTipOpen = false;
										handleInput('');
										inputEl?.focus();
									}}
									class="interior-clear-btn inline-flex min-h-8 min-w-8 items-center justify-center [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
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
								class="z-60 rounded-md border border-line-strong bg-surface px-2 py-1 text-xs/5 font-medium text-fg shadow-md"
							>
								Clear search
							</Tooltip.Content>
						</Tooltip.Portal>
					</Tooltip.Root>
				</Tooltip.Provider>
			{:else if shortcutsEnabled}
				<kbd class="interior-search-kbd" aria-hidden="true">/</kbd>
			{/if}
		</div>

		{#if shellCommand}
			<button
				type="button"
				onclick={openWorkspacePalette}
				aria-label="Open workspace command palette"
				aria-keyshortcuts="Meta+K Control+K"
				aria-haspopup="dialog"
				title="Open workspace command palette"
				class="interior-workspace-btn"
			>
				<span>Workspace</span>
				<kbd aria-hidden="true">{cmdShortcutLabel}</kbd>
			</button>
		{/if}
	</div>
	<NotificationsButton />
</div>

<style>
	.interior-search-tools {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
		max-width: 100%;
	}
	.interior-cmdk-shell {
		position: relative;
		display: flex;
		align-items: center;
		gap: 4px;
		width: 100%;
		min-width: 0;
		min-height: max(2.5rem, var(--triage-control-height, 36px));
		padding: 2px;
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-well, var(--surface-raised));
	}
	.interior-cmdk-shell:has(.interior-search-input:focus) {
		border-color: var(--focus);
	}
	.interior-local-field {
		display: flex;
		flex: 1 1 auto;
		align-items: center;
		gap: 8px;
		min-width: 0;
		height: 100%;
		padding-inline: 8px 4px;
	}
	.interior-local-field :global(.interior-search-ico) {
		color: var(--exec-muted, var(--fg-muted));
	}
	.interior-search-input {
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
	.interior-search-input::placeholder {
		color: var(--exec-muted, var(--fg-muted));
		opacity: 1;
	}
	.interior-search-input::-webkit-search-cancel-button {
		-webkit-appearance: none;
		appearance: none;
	}
	.interior-search-kbd {
		flex: none;
		padding: 1px 6px;
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		border-radius: 4px;
		color: var(--exec-muted, var(--fg-muted));
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.75rem;
		line-height: 1.3;
	}
	.interior-clear-btn {
		border-radius: var(--exec-sm-radius, 6px);
		color: var(--exec-muted, var(--fg-muted));
		cursor: pointer;
	}
	.interior-clear-btn:hover {
		color: var(--exec-ink, var(--fg));
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
	}
	.interior-workspace-btn {
		position: relative;
		display: inline-flex;
		flex: 0 0 auto;
		align-items: center;
		gap: 6px;
		min-height: 2.125rem;
		padding-inline: 10px;
		border-radius: calc(var(--exec-ctrl-radius, 8px) - 2px);
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.8125rem;
		font-weight: 500;
		white-space: nowrap;
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.interior-workspace-btn::before {
		content: '';
		position: absolute;
		inset-inline-start: -3px;
		inset-block: 6px;
		border-inline-start: 1px solid var(--exec-border-hi, var(--line-strong));
	}
	.interior-workspace-btn:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
		color: var(--exec-ink, var(--fg));
	}
	.interior-workspace-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -1px;
	}
	.interior-workspace-btn kbd {
		color: var(--exec-muted, var(--fg-muted));
		font-family: inherit;
		font-size: 0.75rem;
		font-weight: 600;
	}
	@media (max-width: 720px), (pointer: coarse) {
		.interior-cmdk-shell {
			min-height: 2.75rem;
		}
		.interior-workspace-btn {
			min-height: 2.5rem;
			min-width: 2.5rem;
		}
		.interior-search-kbd,
		.interior-workspace-btn kbd {
			display: none;
		}
		.interior-search-input {
			font-size: 1rem;
		}
	}
	@media (forced-colors: active) {
		.interior-cmdk-shell {
			border: 1px solid ButtonText;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.interior-workspace-btn {
			transition: none;
		}
	}
</style>
