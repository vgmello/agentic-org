<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import type { Connection, OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import type { InboxItem } from '$lib/modules/inbox/types';
	import type { Conversation } from '$lib/modules/messaging/types';
	import NewMenu, { type NewMenuAction } from './NewMenu.svelte';
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
	import { hasBlockingOverlay, isChatHashShortcut } from './command-palette-shortcuts';
	import { findTessDmId, resolveMemberDmId } from './shell-chat-nav';
	import type { ShellAiAskOptions } from '$lib/platform/workspace/shell-ai';
	import { useShellData } from './shell-data.svelte';
	import { isEditableTarget, singleKeyShortcutsOn } from '$lib/platform/workspace/shortcuts';
	import { uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';
	import type CommandPaletteModalType from './CommandPaletteModal.svelte';

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

	const shellData = useShellData();
	const organizationId = $derived(shellData.organizationId);
	const isChat = $derived(page.url.pathname === '/chat' || page.url.pathname.startsWith('/chat/'));
	const openInboxCount = $derived(inboxItems.filter((item) => item.state === 'open').length);

	let chatConversations = $state.raw<Conversation[] | undefined>(undefined);
	let chatConversationsOrgId = $state<string | undefined>(undefined);
	let chatLoadSeq = 0;
	let chatLoadAbort: AbortController | undefined;
	let destroyed = false;

	let PaletteModal = $state.raw<typeof CommandPaletteModalType | undefined>(undefined);
	let modalRef = $state<ReturnType<typeof CommandPaletteModalType> | undefined>(undefined);
	let modalLoading = false;

	onDestroy(() => {
		destroyed = true;
		chatLoadAbort?.abort();
		chatLoadAbort = undefined;
	});

	async function ensureModalLoaded() {
		if (PaletteModal || modalLoading) return;
		modalLoading = true;
		try {
			const mod = await import('./CommandPaletteModal.svelte');
			if (!destroyed) PaletteModal = mod.default;
		} catch {
			if (!destroyed && !PaletteModal) open = false;
		} finally {
			modalLoading = false;
		}
	}

	async function loadChatConversations(targetOrgId = organizationId) {
		if (!targetOrgId) return;
		chatLoadAbort?.abort();
		const controller = new AbortController();
		chatLoadAbort = controller;
		const seq = ++chatLoadSeq;
		try {
			const { chatApi } = await import('$lib/modules/messaging/api');
			const snap = await chatApi.snapshot(targetOrgId, controller.signal);
			if (
				destroyed ||
				controller.signal.aborted ||
				seq !== chatLoadSeq ||
				targetOrgId !== organizationId
			) {
				return;
			}
			if (!activeEntryId && open && query.trim().length > 0 && safeActiveIdx >= 0) {
				const currentEntry = entries[safeActiveIdx];
				if (currentEntry) activeEntryId = currentEntry.id;
			}
			chatConversations = snap.state.conversations;
			chatConversationsOrgId = targetOrgId;
		} catch {
			// Unserved or offline chat falls back to snapshot-derived channel suggestions.
		} finally {
			if (chatLoadAbort === controller) chatLoadAbort = undefined;
		}
	}

	let open = $state(false);
	let modalMode = $state<'cmd' | 'ai'>('cmd');
	let query = $state('');
	const isChannelQuery = $derived(query.trimStart().startsWith('#'));
	let activeIdx = $state(0);
	let activeEntryId = $state<string | undefined>(undefined);
	let skipRestoreFocus = $state(false);

	function triggerNewAction(kind: NewMenuAction, fallbackHref?: string) {
		if (newMenu) {
			newMenu.runAction(kind);
			return;
		}
		if (fallbackHref) void goto(resolve(fallbackHref as '/'));
	}

	function handoffToTess(prompt: string, opts?: ShellAiAskOptions) {
		closeModal(false);
		onAskTess?.(prompt, opts);
	}

	const channelEntries = $derived.by(() =>
		buildChannelEntries(chatConversations, chatConversationsOrgId === organizationId, snapshot)
	);

	const entries = $derived.by<CommandEntry[]>(() => {
		void offices;
		void ownerId;
		if (!open) return [];
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

	function scrollActiveIntoView(idx: number) {
		if (!open || idx < 0) return;
		const targetId = `exec-cmdk-opt-${idx}`;
		void tick().then(() => {
			if (!destroyed && open) {
				document.getElementById(targetId)?.scrollIntoView({ block: 'nearest' });
			}
		});
	}

	function setActiveIndex(idx: number) {
		activeIdx = idx;
		activeEntryId = entries[idx]?.id;
		scrollActiveIntoView(idx);
	}

	function resetActive() {
		activeIdx = 0;
		activeEntryId = undefined;
	}

	function resetModalState() {
		query = '';
		modalMode = 'cmd';
		resetActive();
	}

	export function openPalette(prefill?: string) {
		if (offline) return;
		if (prefill && prefill.trim().length > 0) openModalWithQuery(prefill.trim());
		else openModal();
	}

	async function navigateToChatConversation(conversationId?: string) {
		const base = resolve('/chat');
		if (conversationId) {
			// eslint-disable-next-line svelte/no-navigation-without-resolve -- base passes through resolve() while query string stays intact
			await goto(`${base}?c=${encodeURIComponent(conversationId)}`);
			return;
		}
		await goto(base);
	}

	export async function openTessChat() {
		const tessDmId = organizationId ? await findTessDmId(organizationId, snapshot) : undefined;
		await navigateToChatConversation(tessDmId);
	}

	export async function openMemberDm(memberId: string) {
		const conversationId = organizationId
			? await resolveMemberDmId(organizationId, memberId)
			: undefined;
		await navigateToChatConversation(conversationId);
	}

	function openModal() {
		skipRestoreFocus = false;
		open = true;
		resetActive();
		void ensureModalLoaded();
		void loadChatConversations(organizationId);
	}

	function openModalWithQuery(nextQuery: string) {
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
		closeModal(!entry.href);
		if (entry.action) entry.action();
		else if (entry.href) void goto(resolve(entry.href as '/'));
	}

	function toggleCommandPaletteFromShortcut() {
		if (open) {
			closeModal();
			return;
		}
		if (offline) return;
		openModal();
	}

	function toggleTessFromShortcut() {
		if (open) {
			modalMode = modalMode === 'cmd' ? 'ai' : 'cmd';
			modalRef?.focusInput();
			return;
		}
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
			onToggleTessExpand?.();
			return true;
		}
		return false;
	}

	function handleSingleKeyShortcut(event: KeyboardEvent) {
		if (isChatHashShortcut(event, isChat)) {
			event.preventDefault();
			if (!offline) openModalWithQuery('#');
			return;
		}
		if (event.metaKey || event.ctrlKey || event.altKey) return;
		if (connection && event.key.toLowerCase() === 'n') {
			event.preventDefault();
			newMenu?.openMenu();
			return;
		}
		if (event.key === '/' && !event.shiftKey) {
			event.preventDefault();
			if (!offline) openModal();
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

	function stepActiveIndex(delta: 1 | -1) {
		if (entries.length === 0) return;
		const defaultIdx = delta === 1 ? 0 : entries.length - 1;
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
		if (open && safeActiveIdx >= 0 && entries[safeActiveIdx]) {
			event.preventDefault();
			runEntry(entries[safeActiveIdx]);
		}
	}

	function handleInputKeydown(event: KeyboardEvent) {
		if (event.isComposing || event.keyCode === 229) return;
		if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
			event.preventDefault();
			stepActiveIndex(event.key === 'ArrowDown' ? 1 : -1);
		} else if (event.key === 'Enter') {
			handleEnterKey(event);
		}
	}

	function handleHashQueryTyped() {
		if (chatConversationsOrgId !== organizationId) {
			void loadChatConversations(organizationId);
		}
	}
</script>

<svelte:window onkeydown={handleWindowKeydown} />

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
