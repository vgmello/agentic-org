<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { afterNavigate, goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import Search from '@lucide/svelte/icons/search';
	import type { Connection, OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import type { InboxItem } from '$lib/modules/inbox/types';
	import type { Conversation } from '$lib/modules/messaging/types';
	import NewMenu, { type NewMenuAction } from './NewMenu.svelte';
	import NotificationsButton from './NotificationsButton.svelte';
	import {
		buildActionEntries,
		buildAskTessEntries,
		buildChannelEntries,
		buildDynamicEntries,
		buildNavigateEntries,
		buildWorkspaceEntries,
		computeCommandEntries,
		type CommandEntry
	} from './command-palette-entries';
	import type { ShellAiAskOptions } from './shell-ai';
	import { useShellData } from './shell-data.svelte';
	import { isEditableTarget, singleKeyShortcutsOn } from './shortcuts';
	import { uiPrefs } from './ui-preferences.svelte';
	import type CommandPaletteModalType from './CommandPaletteModal.svelte';
	import type CommandPaletteResultsType from './CommandPaletteResults.svelte';
	import './executive-topbar.css';

	let {
		connection,
		snapshot,
		items = [],
		inboxItems = [],
		offices = [],
		ownerId = 'owner',
		offline = false,
		onToggleTess,
		onToggleTessExpand,
		onOpenShortcuts,
		onAskTess
	}: {
		connection?: Connection;
		snapshot?: OrgSnapshot;
		items?: WorkItemSummary[];
		inboxItems?: readonly InboxItem[];
		offices?: OfficeInfo[];
		ownerId?: string;
		offline?: boolean;
		onToggleTess?: () => void;
		onToggleTessExpand?: () => void;
		onOpenShortcuts?: () => void;
		onAskTess?: (prompt: string, opts?: ShellAiAskOptions) => void;
	} = $props();

	let newMenu = $state<ReturnType<typeof NewMenu> | undefined>();
	export function openNewTask() {
		newMenu?.runAction('task');
	}

	const kbdClass = 'exec-cmdk-foot-kbd';
	const shellData = useShellData();
	const organizationId = $derived(shellData.organizationId);
	const isHome = $derived(page.url.pathname === '/home');
	const isChat = $derived(page.url.pathname === '/chat' || page.url.pathname.startsWith('/chat/'));
	const openInboxCount = $derived(inboxItems.filter((item) => item.state === 'open').length);

	let chatConversations = $state.raw<Conversation[] | undefined>(undefined);
	let chatConversationsOrgId = $state<string | undefined>(undefined);
	let chatLoadSeq = 0;
	let destroyed = false;

	let PaletteModal = $state.raw<typeof CommandPaletteModalType | undefined>(undefined);
	let modalRef = $state<ReturnType<typeof CommandPaletteModalType> | undefined>(undefined);
	let modalLoading = false;

	let PaletteResults = $state.raw<typeof CommandPaletteResultsType | undefined>(undefined);
	let resultsLoading = false;

	onDestroy(() => {
		destroyed = true;
	});

	$effect(() => {
		void offices;
		void ownerId;
	});

	async function ensureModalLoaded() {
		if (PaletteModal || modalLoading) return;
		modalLoading = true;
		try {
			const mod = await import('./CommandPaletteModal.svelte');
			if (!destroyed) PaletteModal = mod.default;
		} catch {
			// User can retry via Cmd+K.
		} finally {
			modalLoading = false;
		}
	}

	async function ensureResultsLoaded() {
		if (PaletteResults || resultsLoading) return;
		resultsLoading = true;
		try {
			const mod = await import('./CommandPaletteResults.svelte');
			if (!destroyed) PaletteResults = mod.default;
		} catch {
			// User can retry on focus.
		} finally {
			resultsLoading = false;
		}
	}

	async function loadChatConversations(targetOrgId = organizationId) {
		if (!targetOrgId) return;
		const seq = ++chatLoadSeq;
		try {
			const { chatApi } = await import('$lib/modules/messaging/api');
			const snap = await chatApi.snapshot(targetOrgId);
			if (destroyed || seq !== chatLoadSeq || targetOrgId !== organizationId) return;
			if (!activeEntryId && (open || inlineOpen) && query.trim().length > 0 && safeActiveIdx >= 0) {
				const currentEntry = entries[safeActiveIdx];
				if (currentEntry) activeEntryId = currentEntry.id;
			}
			chatConversations = snap.state.conversations;
			chatConversationsOrgId = targetOrgId;
		} catch {
			// Unserved or offline chat falls back to snapshot-derived channel suggestions.
		}
	}

	let open = $state(false);
	let modalMode = $state<'cmd' | 'ai'>('cmd');
	let inlineOpen = $state(false);
	let query = $state('');
	const isChannelQuery = $derived(query.trimStart().startsWith('#'));
	let activeIdx = $state(0);
	let activeEntryId = $state<string | undefined>(undefined);
	let homeInputEl = $state<HTMLInputElement | null>(null);
	let homeShellEl = $state<HTMLDivElement | null>(null);
	let skipRestoreFocus = $state(false);

	function triggerNewAction(kind: NewMenuAction, fallbackHref?: string) {
		if (newMenu) {
			newMenu.runAction(kind);
			return;
		}
		if (fallbackHref) void goto(resolve(fallbackHref as '/'));
	}

	function handoffToTess(prompt: string, opts?: ShellAiAskOptions) {
		closeInline(true);
		closeModal(false);
		onAskTess?.(prompt, opts);
	}

	const channelEntries = $derived.by(() =>
		buildChannelEntries(chatConversations, chatConversationsOrgId === organizationId, snapshot)
	);

	const entries = $derived.by<CommandEntry[]>(() => {
		if (!open && !inlineOpen) return [];
		const baseList: CommandEntry[] = [
			...buildActionEntries(triggerNewAction),
			...buildAskTessEntries(handoffToTess),
			...buildNavigateEntries(openInboxCount),
			...buildWorkspaceEntries({
				navCollapsed: uiPrefs.navCollapsed,
				showTessPill: uiPrefs.showTessPill,
				onToggleNav: () => uiPrefs.toggleNavCollapsed(),
				onToggleTessPill: () => uiPrefs.toggleShowTessPill(),
				onOpenShortcuts,
				onSetTheme: (id) => uiPrefs.setTheme(id)
			})
		];
		const dynamicEntries = query.trim() ? buildDynamicEntries(snapshot, inboxItems, items) : [];
		return computeCommandEntries({
			query,
			isChat,
			modalOpen: open,
			channelEntries,
			baseList,
			dynamicEntries,
			onOpenModalWithQuery: openModalWithQuery,
			onAskTess: handoffToTess
		});
	});

	const safeActiveIdx = $derived.by(() => {
		if (entries.length === 0) return -1;
		if (activeEntryId) return entries.findIndex((e) => e.id === activeEntryId);
		return Math.min(activeIdx, entries.length - 1);
	});
	const activeOptionId = $derived(
		open && safeActiveIdx >= 0 ? `exec-cmdk-opt-${safeActiveIdx}` : undefined
	);
	const inlineListboxVisible = $derived(isHome && inlineOpen && entries.length > 0);
	const inlineActiveOptionId = $derived(
		inlineListboxVisible && safeActiveIdx >= 0 ? `exec-cmdk-home-opt-${safeActiveIdx}` : undefined
	);

	function setActiveIndex(idx: number) {
		activeIdx = idx;
		activeEntryId = entries[idx]?.id;
	}

	function resetActive() {
		activeIdx = 0;
		activeEntryId = undefined;
	}

	afterNavigate(() => {
		if (inlineOpen) closeInline(true);
	});

	function resetModalState() {
		query = '';
		modalMode = 'cmd';
		resetActive();
	}

	function closeInline(clearQuery = true) {
		inlineOpen = false;
		if (clearQuery) query = '';
		resetActive();
	}

	function openInline() {
		open = false;
		inlineOpen = true;
		resetActive();
		void ensureResultsLoaded();
		void loadChatConversations(organizationId);
		void tick().then(() => homeInputEl?.focus());
	}

	export function openPalette(prefill?: string) {
		if (prefill && prefill.trim().length > 0) openModalWithQuery(prefill.trim());
		else openModal();
	}

	function isTessMember(m: NonNullable<OrgSnapshot>['members'][number]): boolean {
		return (
			m.name.toLowerCase().includes('tess') ||
			m.agent?.messaging_name?.toLowerCase() === 'tess' ||
			Boolean(m.agent?.role?.toLowerCase().includes('chief of staff'))
		);
	}

	async function findTessDmId(orgId: string): Promise<string | undefined> {
		try {
			const { chatApi } = await import('$lib/modules/messaging/api');
			const snap = await chatApi.snapshot(orgId);
			const tessMember = (snapshot?.members ?? []).find((m) => isTessMember(m));
			const tessDm = snap.state.conversations.find(
				(c) =>
					c.kind === 'dm' &&
					!c.archived &&
					(Boolean(tessMember && c.members.includes(tessMember.id)) ||
						Boolean(c.name?.toLowerCase().includes('tess')))
			);
			return tessDm?.id;
		} catch {
			return undefined;
		}
	}

	export async function openTessChat() {
		const tessDmId = organizationId ? await findTessDmId(organizationId) : undefined;
		if (tessDmId) {
			const base = resolve('/chat');
			// eslint-disable-next-line svelte/no-navigation-without-resolve -- base passes through resolve() while query string stays intact
			await goto(`${base}?c=${encodeURIComponent(tessDmId)}`);
			return;
		}
		await goto(resolve('/chat'));
	}

	function openModal() {
		closeInline(false);
		skipRestoreFocus = false;
		open = true;
		resetActive();
		void ensureModalLoaded();
		void loadChatConversations(organizationId);
	}

	function openModalWithQuery(nextQuery: string) {
		closeInline(false);
		skipRestoreFocus = false;
		query = nextQuery;
		open = true;
		resetActive();
		void ensureModalLoaded().then(() => modalRef?.focusInput(true));
		void loadChatConversations(organizationId);
	}

	function closeModal(restoreFocus = true) {
		skipRestoreFocus = !restoreFocus;
		open = false;
		resetModalState();
	}

	function runEntry(entry: CommandEntry) {
		if (inlineOpen) closeInline(true);
		else closeModal(!entry.href);
		if (entry.action) entry.action();
		else if (entry.href) void goto(resolve(entry.href as '/'));
	}

	function handleWindowPointerDown(event: PointerEvent) {
		if (!inlineOpen) return;
		if (event.target instanceof Node && homeShellEl?.contains(event.target)) return;
		closeInline(false);
	}

	function hasBlockingOverlay(target: EventTarget | null): boolean {
		const overlaySelector =
			'[role="menu"], [role="dialog"]:not(.work-reading), [role="alertdialog"], [role="listbox"], aside[aria-label="Thread"], aside[aria-label="Members"], [data-chat-panel], [data-slot="sheet-content"]';
		if (document.querySelector(overlaySelector)) return true;
		return (
			target instanceof HTMLElement &&
			Boolean(
				target.closest(
					'[role="menu"], [role="dialog"], [role="alertdialog"], [role="listbox"], aside[aria-label], [data-chat-panel], [data-slot="sheet-content"]'
				)
			)
		);
	}

	function toggleCommandPaletteFromShortcut() {
		if (open) {
			closeModal();
			return;
		}
		if (isHome && inlineOpen && document.activeElement === homeInputEl) {
			closeInline(false);
			homeInputEl?.blur();
			return;
		}
		if (isHome) openInline();
		else openModal();
	}

	function toggleTessFromShortcut() {
		if (open) {
			modalMode = modalMode === 'cmd' ? 'ai' : 'cmd';
			modalRef?.focusInput();
			return;
		}
		if (inlineOpen) closeInline(false);
		onToggleTess?.();
	}

	function handleModShortcut(event: KeyboardEvent): boolean {
		if (!event.metaKey && !event.ctrlKey) return false;
		const lower = event.key.toLowerCase();
		if (lower === 'k') {
			event.preventDefault();
			toggleCommandPaletteFromShortcut();
			return true;
		}
		if (event.key === '/') {
			event.preventDefault();
			toggleTessFromShortcut();
			return true;
		}
		if (event.shiftKey && (lower === 'e' || lower === 'm')) {
			event.preventDefault();
			if (open) closeModal(false);
			if (inlineOpen) closeInline(false);
			onToggleTessExpand?.();
			return true;
		}
		return false;
	}

	function isChatHashShortcut(event: KeyboardEvent): boolean {
		if (!isChat || event.key !== '#' || event.metaKey) return false;
		const altGr = Boolean(event.getModifierState?.('AltGraph'));
		return altGr || (!event.ctrlKey && !event.altKey);
	}

	function handleSingleKeyShortcut(event: KeyboardEvent) {
		if (isChatHashShortcut(event)) {
			event.preventDefault();
			openModalWithQuery('#');
			return;
		}
		if (event.metaKey || event.ctrlKey || event.altKey) return;
		if (!inlineOpen && connection && event.key.toLowerCase() === 'n') {
			event.preventDefault();
			newMenu?.openMenu();
			return;
		}
		if (event.key === '/' && !event.shiftKey) {
			event.preventDefault();
			if (isHome) openInline();
			else openModal();
		}
	}

	function handleWindowKeydown(event: KeyboardEvent) {
		if (event.isComposing || event.keyCode === 229) return;
		if (handleModShortcut(event)) return;
		const blocked =
			open || event.repeat || isEditableTarget(event.target) || hasBlockingOverlay(event.target);
		if (blocked) return;
		if (event.key === '?' && !event.metaKey && !event.ctrlKey && !event.altKey) {
			event.preventDefault();
			onOpenShortcuts?.();
			return;
		}
		if (!singleKeyShortcutsOn()) return;
		handleSingleKeyShortcut(event);
	}

	$effect(() => {
		if (!open && !inlineListboxVisible) return;
		if (entries.length === 0 || safeActiveIdx < 0) return;
		void query;
		const targetId = inlineListboxVisible
			? `exec-cmdk-home-opt-${safeActiveIdx}`
			: `exec-cmdk-opt-${safeActiveIdx}`;
		let cancelled = false;
		void tick().then(() => {
			if (!cancelled) {
				document.getElementById(targetId)?.scrollIntoView({ block: 'nearest' });
			}
		});
		return () => {
			cancelled = true;
		};
	});

	function stepActiveIndex(delta: 1 | -1) {
		const defaultIdx = delta === 1 ? 0 : entries.length - 1;
		if (isHome && !open && !inlineOpen) {
			inlineOpen = true;
			void ensureResultsLoaded();
			if (entries.length > 0) setActiveIndex(defaultIdx);
			else resetActive();
			return;
		}
		if (entries.length === 0) return;
		const next =
			safeActiveIdx < 0 ? defaultIdx : (safeActiveIdx + delta + entries.length) % entries.length;
		setActiveIndex(next);
	}

	function handleEnterKey(event: KeyboardEvent) {
		if (open && modalMode === 'ai') {
			const q = query.trim();
			if (q) {
				event.preventDefault();
				handoffToTess(q);
			}
			return;
		}
		const canRun = open || (isHome && !open && inlineOpen);
		if (canRun && safeActiveIdx >= 0 && entries[safeActiveIdx]) {
			event.preventDefault();
			runEntry(entries[safeActiveIdx]);
		}
	}

	function exitHomeSearch() {
		const heading = document.querySelector<HTMLElement>('#shell-page-title, main h1, main h2');
		if (heading) {
			if (!heading.hasAttribute('tabindex')) heading.tabIndex = -1;
			heading.focus({ preventScroll: true });
		} else {
			homeInputEl?.blur();
		}
	}

	function handleInlineEscape(event: KeyboardEvent) {
		event.preventDefault();
		event.stopPropagation();
		if (inlineOpen) {
			closeInline(false);
			if (query.length === 0) exitHomeSearch();
			return;
		}
		exitHomeSearch();
	}

	function handleInputKeydown(event: KeyboardEvent) {
		if (event.isComposing || event.keyCode === 229) return;
		if (event.key === 'Tab' && isHome && !open && inlineOpen) {
			inlineOpen = false;
			return;
		}
		if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
			event.preventDefault();
			stepActiveIndex(event.key === 'ArrowDown' ? 1 : -1);
		} else if (event.key === 'Enter') {
			handleEnterKey(event);
		} else if (event.key === 'Escape' && isHome && !open) {
			handleInlineEscape(event);
		}
	}

	function handleHashQueryTyped() {
		if (chatConversationsOrgId !== organizationId) {
			void loadChatConversations(organizationId);
		}
	}
</script>

<svelte:window onkeydown={handleWindowKeydown} onpointerdown={handleWindowPointerDown} />

{#if isHome}
	<header class="exec-topbar exec-home-topbar relative z-30 shrink-0 overflow-visible px-4 sm:px-6">
		<div
			bind:this={homeShellEl}
			class="exec-home-cmdk relative flex min-w-0 w-full max-w-[640px] flex-col"
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
						onfocus={() => {
							inlineOpen = true;
							resetActive();
							void ensureResultsLoaded();
							void loadChatConversations(organizationId);
						}}
						oninput={(event) => {
							inlineOpen = true;
							resetActive();
							void ensureResultsLoaded();
							if (event.currentTarget.value.trimStart().startsWith('#')) {
								handleHashQueryTyped();
							}
						}}
						onkeydown={handleInputKeydown}
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
								onSelectIndex={setActiveIndex}
								onRunEntry={runEntry}
								onAskTess={handoffToTess}
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
{/if}
{#if connection}
	<NewMenu bind:this={newMenu} {connection} headless />
{/if}

{#if PaletteModal}
	{@const ModalComp = PaletteModal}
	<ModalComp
		bind:this={modalRef}
		bind:open
		bind:modalMode
		bind:query
		bind:skipRestoreFocus
		{entries}
		{isChannelQuery}
		{safeActiveIdx}
		{activeOptionId}
		onResetModalState={resetModalState}
		onResetActive={resetActive}
		onHashQueryTyped={handleHashQueryTyped}
		onInputKeydown={handleInputKeydown}
		onSelectIndex={setActiveIndex}
		onRunEntry={runEntry}
		onAskTess={handoffToTess}
	/>
{/if}
