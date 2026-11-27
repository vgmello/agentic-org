<script lang="ts">
	import { onMount, tick, untrack } from 'svelte';
	import { page } from '$app/state';
	import { beforeNavigate, goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	import { shellApi } from '$lib/modules/shell/api';
	import {
		type CommandEnv,
		browseChannels,
		chatContext,
		composerState,
		conversationView,
		messageRows,
		pinsIn,
		parseSearch,
		searchMessages,
		sidebar,
		titleOf,
		typingWords,
		unreadIn,
		type ChatMessage,
		type ChatSnapshot,
		type ChatView
	} from '$lib/modules/messaging';
	import { ChatSidebar } from '$lib/modules/messaging/ui';
	import type { OrgSnapshot } from '$lib/proto';
	import { chatApi, sender } from '../../api';
	import { conversationSubtitle, copyFreshness, urlLocation } from '../../helpers';
	import type { ChatLocation, ChatPanelKind, PendingMessage } from '../../types';
	import { createChatSnapshotLoader } from './chat-loader';
	import { ChatOverlayController } from './chat-overlays.svelte';
	import { createChatMessageActions } from './chat-workspace-actions';
	import {
		buildChatPlaceUrl,
		computeQueryNavigation,
		findNewMarkerMessageId,
		resolveChatLocation,
		scrollMessageIntoView
	} from './chat-workspace-helpers';
	import ChatConversationColumn from './ChatConversationColumn.svelte';
	import ChatSidePanels from './ChatSidePanels.svelte';
	import ChatWorkspaceDialogs from './ChatWorkspaceDialogs.svelte';

	/** Chat (chat-ux.md): channels, DMs and threads where agents and people talk. */
	let { organizationId, snapshot }: { organizationId: string; snapshot: OrgSnapshot } = $props();

	const componentId = $props.id();
	const conversationsId = `${componentId}-conversations`;
	const send = $derived(sender(organizationId));

	let chat = $state.raw<ChatSnapshot | undefined>();
	let loadError = $state<string | undefined>();
	let now = $state(new Date().toISOString());
	let inboxCount = $state<number | undefined>();

	const ctx = $derived(chatContext(snapshot, { now, directory: chat?.directory }));
	const cannotRun = $derived(chat?.cannot_run ?? []);
	const chatState = $derived(chat?.state);
	const rows = $derived(chatState ? messageRows(chatState, ctx) : undefined);
	const bar = $derived(chatState ? sidebar(chatState, ctx) : undefined);
	const officeRunning = $derived(chat?.office_running ?? true);
	const copyAge = $derived(copyFreshness(chat?.copy_at, now));

	let view = $state<ChatView>({ type: 'mentions' });
	let compactPanels = $state(false);
	let conversationEl = $state<HTMLElement | undefined>();
	let panel = $state<{ focusClose: () => void } | undefined>();
	let dialogs = $state<{ dismissOpenDialogs: () => void } | undefined>();
	let query = $state('');
	let readMarker = $state<string | undefined>();
	let showMuted = $state(false);
	let openDigest = $state<string | undefined>();
	let started = false;
	let lastSearch = '';
	let pendingPlace = $state.raw<URL>();
	let settledPlace = $state('Chat');
	let listEl = $state<HTMLElement | undefined>();
	let chatEl = $state<HTMLElement | undefined>();
	let heading = $state<HTMLHeadingElement | undefined>();
	let sidebarToggle = $state<HTMLButtonElement | HTMLAnchorElement | null>(null);
	let conversationNavigation = $state<
		{ focusSelected: (preferred?: HTMLElement) => void } | undefined
	>();
	let pending = $state<PendingMessage[]>([]);

	const ui: ChatOverlayController = new ChatOverlayController({
		getCompactPanels: () => compactPanels,
		getPanelKind: (): ChatPanelKind | undefined => panelKind,
		getOfficeRunning: () => officeRunning,
		getConversationEl: () => conversationEl,
		getListEl: () => listEl,
		getHeading: () => heading,
		getSidebarToggle: () => sidebarToggle,
		getPanel: () => panel,
		getDialogs: () => dialogs,
		getNavigation: () => conversationNavigation,
		onWritePlace: () => writePlace(),
		onSelectConversation: (id, navigate = true) => select({ type: 'conversation', id }, navigate),
		onResetSearchForThread: (conversationId) => {
			if (chatState && (searching || view.type !== 'conversation' || view.id !== conversationId))
				select({ type: 'conversation', id: conversationId }, false);
		}
	});

	$effect.pre(() => {
		if (!ui.sidebarInline) return;
		untrack(() => {
			const origin = sidebarToggle;
			if (!origin || document.activeElement !== origin) return;
			void tick().then(() => {
				if (
					ui.sidebarInline &&
					(document.activeElement === origin || document.activeElement === document.body)
				)
					conversationNavigation?.focusSelected();
			});
		});
	});

	const bridgePreview = $derived(page.url.searchParams.get('surface') === 'bridge');
	const viewId = $derived(view.type === 'conversation' ? view.id : undefined);
	const conversation = $derived(
		viewId && chatState ? chatState.conversations.find((c) => c.id === viewId) : undefined
	);
	const searching = $derived(query.trim() !== '' && page.url.searchParams.has('q'));
	const convView = $derived(
		conversation && chatState ? conversationView(chatState, ctx, conversation.id) : undefined
	);
	const pins = $derived(conversation && chatState ? pinsIn(chatState, conversation.id) : []);
	const root: ChatMessage | undefined = $derived(
		ui.threadRoot && chatState ? chatState.messages.find((m) => m.id === ui.threadRoot) : undefined
	);
	const threadConversation = $derived(
		root && chatState
			? chatState.conversations.find((c) => c.id === root.conversation_id)
			: undefined
	);
	const channelRowLive = (message: ChatMessage): 'off' | undefined =>
		root && message.thread_root_id === root.id ? 'off' : undefined;
	const panelKind: ChatPanelKind | undefined = $derived(
		root && threadConversation
			? 'thread'
			: ui.showMembers && conversation
				? 'members'
				: ui.showPins && conversation
					? 'pins'
					: undefined
	);

	$effect(() => {
		const target = panel;
		if (!compactPanels || !panelKind || !target) return;
		untrack(() => {
			const origin = document.activeElement;
			if (origin !== document.body && !conversationEl?.contains(origin)) return;
			void tick().then(() => {
				if (
					panel === target &&
					compactPanels &&
					panelKind &&
					(document.activeElement === document.body || document.activeElement === origin)
				)
					target.focusClose();
			});
		});
	});

	const compose = $derived(
		conversation && chatState
			? composerState(chatState, ctx, conversation, officeRunning, cannotRun)
			: undefined
	);
	const typing = $derived(
		conversation && chatState ? typingWords(chatState.typing, ctx, conversation.id, now) : undefined
	);
	const results = $derived(
		chatState && searching ? searchMessages(chatState, ctx, parseSearch(query)) : []
	);
	const subtitle = $derived(conversation ? conversationSubtitle(conversation, snapshot) : '');
	const viewTitle = $derived(
		searching
			? 'Search'
			: view.type === 'mentions'
				? 'Mentions'
				: view.type === 'threads'
					? 'Threads'
					: conversation
						? titleOf(conversation, ctx, ctx.owner)
						: 'Chat'
	);
	const pendingHere = $derived(
		pending.filter(
			(p) => p.conversation_id === conversation?.id && (!p.thread_root_id || p.also_in_channel)
		)
	);
	const env = $derived<CommandEnv>({
		projects: (snapshot.projects ?? []).map((p) => ({ id: p.id, name: p.name })),
		members: ctx.members,
		conversationTitle:
			conversation && chatState ? titleOf(conversation, ctx, ctx.owner) : 'this conversation'
	});
	const browseRows = $derived(
		chatState ? browseChannels(chatState, ctx, snapshot.projects ?? [], snapshot.teams) : []
	);
	const newAt = $derived(findNewMarkerMessageId(readMarker, convView?.shown, ctx.owner));

	const documentPlace = $derived(
		!pendingPlace || page.url.href === pendingPlace.href ? viewTitle : settledPlace
	);

	let mounted = false;
	const loader = createChatSnapshotLoader({
		chatApi,
		getOrganizationId: () => organizationId,
		hasSnapshot: () => Boolean(chat),
		onSnapshot: (snap, nowIso) => {
			now = nowIso;
			chat = snap;
			loadError = undefined;
			if (mounted && !started && snap.state) {
				openLocation(urlLocation(page.url.search));
				lastSearch = page.url.search;
				started = true;
				shellApi.navigation(organizationId).then(
					(n) => (mounted ? (inboxCount = n.inboxOpen ?? undefined) : undefined),
					() => (mounted ? (inboxCount = undefined) : undefined)
				);
			}
		},
		onError: (msg) => (loadError = msg)
	});
	const load = async (): Promise<void> => {
		await loader.load();
	};

	onMount(() => {
		mounted = true;
		const media = window.matchMedia('(width < 1280px)');
		const syncCompactPanels = () => (compactPanels = media.matches);
		syncCompactPanels();
		media.addEventListener('change', syncCompactPanels);
		void load();
		const stop = chatApi.subscribe(organizationId, () => loader.scheduleLiveReload());
		const timer = setInterval(() => void load(), 30_000);
		return () => {
			mounted = false;
			ui.dispose();
			loader.dispose();
			stop();
			clearInterval(timer);
			media.removeEventListener('change', syncCompactPanels);
		};
	});

	$effect(() => {
		const search = page.url.search;
		const pendingTarget = pendingPlace;
		untrack(() => {
			if (!started || !chatState || search === lastSearch) return;
			if (pendingTarget && search !== pendingTarget.search) return;
			lastSearch = search;
			if (!pendingTarget) openLocation(urlLocation(search));
		});
	});

	function openLocation(loc: ChatLocation) {
		if (!chatState) return;
		const resolved = resolveChatLocation(chatState, loc);
		if (resolved.redirectInbox) {
			void goto(resolve('/inbox'), { replaceState: true });
			return;
		}
		select(resolved.view, false);
		query = resolved.query;
		if (resolved.threadRoot) ui.threadRoot = resolved.threadRoot;
		ui.highlight = resolved.highlight;
		scrollMessageIntoView(resolved.highlight, () => listEl);
	}

	function select(next: ChatView, navigate = true) {
		ui.resetForViewChange();
		query = '';
		view = next;
		showMuted = false;
		openDigest = undefined;
		readMarker = next.type === 'conversation' ? chatState?.read_at[next.id] : undefined;
		if (next.type === 'conversation')
			void tick().then(() => listEl && (listEl.scrollTop = listEl.scrollHeight));
		if (navigate) writePlace();
	}

	const writePlace = (messageId?: string) =>
		navigateToPlace(buildChatPlaceUrl(page.url, view, ui.threadRoot, query, messageId));

	function changeQuery(value: string) {
		const nav = computeQueryNavigation(
			value,
			page.url,
			pendingPlace,
			page.state as Record<string, unknown> & { chatSearchEntry?: true }
		);
		if (nav.kind === 'back') return history.back();
		query = value;
		navigateToPlace(nav.next, { replaceState: nav.replaceState, state: nav.state });
	}

	function navigateToPlace(
		next: URL,
		options: { replaceState?: boolean; state?: typeof page.state } = {}
	) {
		if (next.href === (pendingPlace ?? page.url).href) return;
		settledPlace = viewTitle;
		pendingPlace = next;
		const destination = `${resolve('/chat')}${next.search}`;
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- destination is built from resolve('/chat') + search params
		void goto(destination, {
			...options,
			replaceState: options.replaceState || next.href === page.url.href,
			keepFocus: true,
			noScroll: true
		}).finally(() => {
			if (pendingPlace === next) pendingPlace = undefined;
		});
	}

	function showPinnedMessage(m: ChatMessage) {
		if (compactPanels) ui.showPins = false;
		if (view.type === 'conversation' && view.id === m.conversation_id) {
			ui.highlight = m.id;
			return scrollMessageIntoView(m.id, () => listEl);
		}
		openMessage(m);
	}

	const openMessage = (m: ChatMessage) => {
		openLocation({ conversation: m.conversation_id, message: m.id });
		writePlace(m.id);
	};

	const removePending = (key: number) => (pending = pending.filter((x) => x.key !== key));

	const actions = createChatMessageActions({
		chatApi,
		getOrganizationId: () => organizationId,
		getSend: () => send,
		getCtx: () => ctx,
		getChatState: () => chatState,
		getEnv: () => env,
		getOfficeRunning: () => officeRunning,
		getListEl: () => listEl,
		focusPlace: () => `${view.type}:${viewId ?? ''}:${ui.threadRoot ?? ''}`,
		load,
		pushPending: (item) => {
			pending.push(item);
			return pending[pending.length - 1];
		},
		removePending,
		openCommandForm: (form) => {
			if ('error' in form) {
				toast.error(form.error);
				return false;
			}
			ui.command = form;
			ui.commandOpen = true;
			return true;
		},
		onTurnHeld: (messageId, opener) => {
			ui.rowActivity.hold(messageId, opener, 'dialog');
			ui.commandOpener = opener;
		},
		onSelectConversation: (id) => select({ type: 'conversation', id })
	});

	$effect(() => {
		const c = conversation;
		const s = chatState;
		if (!c || !s) return;
		untrack(() => {
			if (unreadIn(s, ctx, c).unread > 0)
				void actions.act({ method: 'chat.mark_read', conversation_id: c.id }, undefined, true);
		});
	});

	let lastCount = 0;
	$effect(() => {
		const n = (convView?.shown.length ?? 0) + pending.length;
		const c = conversation?.id;
		untrack(() => {
			if (
				listEl &&
				c &&
				n > lastCount &&
				listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight < 200
			)
				void tick().then(() => listEl && (listEl.scrollTop = listEl.scrollHeight));
			lastCount = n;
		});
	});

	beforeNavigate((navigation) => {
		if (navigation.type !== 'popstate' || !ui.hasOpenOverlay()) return;
		navigation.cancel();
		ui.dismissAllOverlays();
	});
</script>

<svelte:window onkeydown={ui.handleWindowKeydown} />
<svelte:head><title>{documentPlace} · Chat · Agentic Org</title></svelte:head>

<!-- At high zoom, keep a readable message area and let the shell scroll vertically. -->
<div
	bind:this={chatEl}
	onfocusin={ui.revealChatFocus}
	class="chat-frame relative flex min-h-0 min-w-0"
>
	{#if loadError}
		<div class="m-8 space-y-3">
			<p class="text-sm/6 text-danger" role="alert">{loadError}</p>
			<Button
				type="button"
				size="sm"
				variant="outline"
				data-testid="chat-retry"
				onclick={() => {
					loadError = undefined;
					void load();
				}}>Retry</Button
			>
		</div>
	{:else if !chatState || !bar || !rows}
		<p class="m-8 text-sm/6 text-fg-muted" role="status">Loading chat…</p>
	{:else}
		<ChatSidebar
			bind:this={conversationNavigation}
			{bar}
			{inboxCount}
			{view}
			{ctx}
			{officeRunning}
			container={chatEl}
			id={conversationsId}
			bind:inline={ui.sidebarInline}
			bind:open={ui.sidebarOpen}
			onCloseFocus={ui.closeSidebarFocus}
			onselect={select}
			onnewchannel={(opener) => ui.startSidebarDialog('channel', opener)}
			onnewmessage={(opener) => ui.startSidebarDialog('dm', opener)}
			onbrowse={ui.openBrowse}
			onopendm={actions.openDm}
		/>

		<ChatConversationColumn
			bind:conversationEl
			bind:listEl
			bind:heading
			bind:sidebarToggle
			bind:showMuted
			bind:openDigest
			inert={(compactPanels && !!panelKind) || ui.sidebarOpen}
			{viewTitle}
			{subtitle}
			{conversation}
			{searching}
			sample={!!chat?.sample}
			sidebarInline={ui.sidebarInline}
			sidebarOpen={ui.sidebarOpen}
			{conversationsId}
			{inboxCount}
			pinCount={pins.length}
			showPins={ui.showPins}
			showMembers={ui.showMembers}
			{query}
			{bridgePreview}
			{officeRunning}
			{copyAge}
			{results}
			{view}
			{bar}
			{convView}
			{chatState}
			{ctx}
			{rows}
			{now}
			{newAt}
			highlight={ui.highlight}
			{typing}
			{compose}
			{cannotRun}
			{pendingHere}
			{actions}
			rowIsActive={ui.rowIsActive}
			onmenu={ui.menuChanged}
			{channelRowLive}
			onOpenSidebar={ui.openSidebar}
			onTogglePins={ui.togglePins}
			onToggleMembers={ui.toggleMembers}
			onQueryChange={changeQuery}
			onOpenThread={ui.openThread}
			onOpenMessage={openMessage}
			onRemovePending={removePending}
		/>

		<ChatSidePanels
			bind:this={panel}
			{root}
			{threadConversation}
			showMembers={ui.showMembers}
			showPins={ui.showPins}
			{conversation}
			{rows}
			sidebarInline={ui.sidebarInline}
			{ctx}
			{chatState}
			{snapshot}
			{pins}
			{now}
			{officeRunning}
			{cannotRun}
			{bridgePreview}
			{pending}
			rowIsActive={ui.rowIsActive}
			{actions}
			onmenu={ui.menuChanged}
			onCloseThread={ui.closeThread}
			onCloseMembers={ui.closeMembers}
			onClosePins={ui.closePins}
			onRenameChannel={() => ui.openChannelDialog('rename')}
			onAddPeople={() => ui.openChannelDialog('members')}
			onShowPinned={showPinnedMessage}
			onRemovePending={removePending}
			onMessageMember={actions.openDm}
		/>
	{/if}
</div>

<ChatWorkspaceDialogs
	bind:this={dialogs}
	{ctx}
	{conversation}
	{officeRunning}
	{browseRows}
	{send}
	{load}
	channelMode={ui.channelMode}
	bind:channelOpen={ui.channelOpen}
	bind:dmOpen={ui.dmOpen}
	bind:browseOpen={ui.browseOpen}
	bind:commandOpen={ui.commandOpen}
	command={ui.command}
	sidebarDialogMode={ui.sidebarDialogMode}
	hasCommandOpener={!!ui.commandOpener}
	onFinishSidebarDialog={ui.finishSidebarDialog}
	onBrowseReturn={ui.browseReturn}
	onBrowseShow={ui.browseShow}
	onReturnCommandFocus={ui.returnCommandFocus}
	onSelectConversation={(id) => select({ type: 'conversation', id })}
/>

<style>
	.chat-frame {
		--chat-header-height: 4rem;
		height: calc(100dvh - var(--chat-header-height));
	}
	@media (width < 640px) {
		.chat-frame {
			--chat-header-height: 3.5rem;
		}
	}
	@media (width < 640px), (height <= 320px) {
		.chat-frame {
			min-height: 28rem;
		}
		/* Include the sticky shell header when scrolling brings focus into view. */
		.chat-frame :global(:focus) {
			scroll-margin-block-start: var(--chat-header-height);
		}
	}
</style>
