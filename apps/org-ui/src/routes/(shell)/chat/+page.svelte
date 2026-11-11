<script lang="ts">
	import { onMount, tick, untrack } from 'svelte';
	import { page } from '$app/state';
	import { beforeNavigate, goto } from '$app/navigation';
	import { toast } from 'svelte-sonner';
	import Search from '@lucide/svelte/icons/search';
	import Users from '@lucide/svelte/icons/users';
	import Info from '@lucide/svelte/icons/info';
	import Unplug from '@lucide/svelte/icons/unplug';
	import Callout from '$lib/components/Callout.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	// D-F14a: temporary route composition root; Jules's F1-02 factory cutover removes this exception.
	import { chatApi, sender } from '$lib/modules/messaging/legacy-api';
	import {
		ChatSidebar,
		NewMessageDialog,
		Composer,
		MessageRow,
		ThreadPanel,
		AskCard,
		ChannelDialog,
		MembersPanel,
		CommandDialog,
		commandForm,
		turnForm,
		type CommandEnv,
		type CommandForm,
		chatContext,
		composerState,
		conversationView,
		dayLabel,
		locate,
		nameOf,
		parseChatLocation,
		parseSearch,
		parseSlash,
		repliesTo,
		searchMessages,
		sidebar,
		titleOf,
		typingWords,
		unreadIn,
		type ChatView
	} from '$lib/modules/messaging';
	import type { ChatCall, ChatMessage, ChatSnapshot, Conversation, MuteMode, Reaction } from '$lib/modules/messaging';
	import { newId } from '$lib/org/commands';
	import { toAppError } from '$lib/office/api';

	/** Chat (docs/specs/chat-ux.md): channels, DMs and threads where agents and people talk. A mock office; always sample data. */
	let { data } = $props();
	const componentId = $props.id();
	const conversationsId = componentId + '-conversations';
	const organizationId = $derived(data.connection.organization.id);
	const snapshot = $derived(data.snapshot);
	const send = $derived(sender(organizationId));

	let chat = $state<ChatSnapshot | undefined>();
	let loadError = $state<string | undefined>();
	let now = $state(new Date().toISOString());
	const ctx = $derived(chatContext(snapshot, { now }));
	const chatState = $derived(chat?.state);
	const bar = $derived(chatState ? sidebar(chatState, ctx) : undefined);
	const officeRunning = $derived(chat?.office_running ?? true);

	let view = $state<ChatView>({ type: 'needs' });
	let threadRoot = $state<string | undefined>();
	let threadOpener: HTMLElement | undefined;
	let showMembers = $state(false);
	let membersOpener: HTMLElement | undefined;
	let compactPanels = $state(false);
	let conversationEl = $state<HTMLElement | undefined>();
	let panel = $state<{ focusClose: () => void } | undefined>();
	let query = $state('');
	let readMarker = $state<string | undefined>();
	let showMuted = $state(false);
	let openDigest = $state<string | undefined>();
	let highlight = $state<string | undefined>();
	let started = false;
	let lastSearch = '';
	let pendingPlace = $state.raw<URL>();
	let documentPlace = $state('Needs you');
	let listEl = $state<HTMLElement | undefined>();
	let chatEl = $state<HTMLElement | undefined>();
	let heading = $state<HTMLHeadingElement | undefined>();
	let sidebarInline = $state(true);
	let sidebarOpen = $state(false);
	let sidebarToggle = $state<HTMLButtonElement | HTMLAnchorElement | null>(null);
	let conversationNavigation = $state<{ focusSelected: (preferred?: HTMLElement) => void } | undefined>();
	// A resize can remove the toggle just after a dialog restores focus to it.
	$effect.pre(() => {
		if (!sidebarInline) return;
		untrack(() => {
			const origin = sidebarToggle;
			if (!origin || document.activeElement !== origin) return;
			void tick().then(() => {
				if (sidebarInline && (document.activeElement === origin || document.activeElement === document.body)) conversationNavigation?.focusSelected();
			});
		});
	});
	let sidebarReturn: 'toggle' | 'selection' | 'dialog' = 'toggle';
	let sidebarDialogMode = $state(false);
	let sidebarDialogSelected = false;
	let sidebarDialogOpener: HTMLElement | undefined;
	function revealFocus(target: HTMLElement) {
		// The shell heading stays fixed while a short, zoomed Chat can scroll vertically.
		const rect = target.getBoundingClientRect();
		const x = rect.x + rect.width / 2;
		const inset = Math.min(1, rect.height / 2);
		const scrollMargin = Number.parseFloat(getComputedStyle(target).scrollMarginBlockStart) || 0;
		const clippedByViewport = rect.top < scrollMargin || rect.bottom > innerHeight;
		const covered = ![rect.top + inset, rect.bottom - inset].every((y) => target.contains(document.elementFromPoint(x, y)));
		if (clippedByViewport || covered) target.scrollIntoView({ block: 'center', inline: 'nearest' });
	}
	function focusVisible(target: HTMLElement | null | undefined) {
		if (!target?.isConnected) return;
		target.focus({ preventScroll: true });
		if (compactPanels) revealFocus(target);
	}
	function revealChatFocus(event: FocusEvent) {
		const target = event.target;
		if (!(target instanceof HTMLElement) || !(conversationEl?.contains(target) || target.closest('[data-chat-panel]'))) return;
		// Native Tab can scroll after focusin; inspect the final position without changing focus.
		requestAnimationFrame(() => {
			if (target.isConnected && document.activeElement === target) revealFocus(target);
		});
	}
	function focusAfterNavigation(selected: boolean) {
		if (compactPanels && panelKind) panel?.focusClose();
		else if (selected) focusVisible(heading);
		else if (sidebarInline) conversationNavigation?.focusSelected();
		else focusVisible(sidebarToggle);
	}
	function closeSidebarFocus() {
		const reason = sidebarReturn;
		sidebarReturn = 'toggle';
		if (reason !== 'dialog') void tick().then(() => focusAfterNavigation(reason === 'selection'));
	}
	function startSidebarDialog(kind: 'channel' | 'dm', opener: HTMLElement) {
		if (!officeRunning) return;
		sidebarDialogMode = true;
		sidebarDialogSelected = false;
		sidebarDialogOpener = opener;
		if (sidebarOpen) { sidebarReturn = 'dialog'; sidebarOpen = false; }
		if (kind === 'channel') openChannelDialog('create'); else dmOpen = true;
	}
	function finishSidebarDialog() {
		const selected = sidebarDialogSelected;
		const opener = sidebarDialogOpener;
		sidebarDialogMode = false;
		sidebarDialogOpener = undefined;
		void tick().then(() => {
			if (sidebarInline) conversationNavigation?.focusSelected(opener);
			else focusAfterNavigation(selected);
		});
	}

	// Previews how a bridge shows asks (CHAT-17): a mock hook like `?mock=`, `?org=` and `?chat=`.
	const bridgePreview = $derived(page.url.searchParams.get('surface') === 'bridge');

	const viewId = $derived(view.type === 'conversation' ? view.id : undefined);
	const conversation = $derived(viewId && chatState ? chatState.conversations.find((c) => c.id === viewId) : undefined);
	const searching = $derived(query.trim() !== '' && page.url.searchParams.has('q'));
	const convView = $derived(conversation && chatState ? conversationView(chatState, ctx, conversation.id) : undefined);
	const root = $derived(threadRoot && chatState ? chatState.messages.find((m) => m.id === threadRoot) : undefined);
	const threadConversation = $derived(root && chatState ? chatState.conversations.find((c) => c.id === root.conversation_id) : undefined);
	// The open Thread owns its replies' live offers; channel copies remain readable.
	function channelRowLive(message: ChatMessage): 'off' | undefined {
		return root && message.thread_root_id === root.id ? 'off' : undefined;
	}
	const panelKind = $derived(root && threadConversation ? 'thread' : showMembers && conversation ? 'members' : undefined);
	$effect(() => {
		const target = panel;
		if (!compactPanels || !panelKind || !target) return;
		untrack(() => {
			const origin = document.activeElement;
			if (origin !== document.body && !conversationEl?.contains(origin)) return;
			void tick().then(() => {
				if (panel === target && compactPanels && panelKind && (document.activeElement === document.body || document.activeElement === origin)) target.focusClose();
			});
		});
	});
	const compose = $derived(conversation && chatState ? composerState(chatState, ctx, conversation, officeRunning) : undefined);
	const typing = $derived(conversation && chatState ? typingWords(chatState.typing, ctx, conversation.id, now) : undefined);
	const results = $derived(chatState && searching ? searchMessages(chatState, ctx, parseSearch(query)) : []);
	const projectOf = (c: Conversation) => {
		if (c.auto?.type === 'project') {
			const id = c.auto.project_id;
			return snapshot.projects?.find((p) => p.id === id)?.name;
		}
		if (c.auto?.type === 'team') return undefined;
		return undefined;
	};
	const followsWords = (c: Conversation) => (c.auto?.type === 'general' ? 'the organization' : c.auto?.type === 'project' ? `the ${projectOf(c) ?? 'project'} project` : c.auto?.type === 'team' ? `the ${snapshot.teams.find((t) => c.auto?.type === 'team' && t.id === c.auto.team_id)?.name ?? ''} team` : undefined);
	const subtitle = $derived(conversation ? `${conversation.members.length} ${conversation.members.length === 1 ? 'member' : 'members'}${conversation.auto ? ` · follows ${followsWords(conversation)}` : ''}` : '');
	const viewTitle = $derived(searching ? 'Search' : view.type === 'needs' ? 'Needs you' : view.type === 'mentions' ? 'Mentions' : view.type === 'threads' ? 'Threads' : conversation ? titleOf(conversation, ctx, ctx.owner) : 'Chat');

	$effect(() => {
		// Let SvelteKit push the new entry before updating its title.
		if (!pendingPlace || page.url.href === pendingPlace.href) documentPlace = viewTitle;
	});

	async function load() {
		try {
			now = new Date().toISOString();
			chat = await chatApi.snapshot(organizationId);
			loadError = undefined;
		} catch (e) {
			if (!chat) loadError = toAppError(e).message;
		}
	}

	onMount(() => {
		const media = window.matchMedia('(width < 1280px)');
		const syncCompactPanels = () => (compactPanels = media.matches);
		syncCompactPanels();
		media.addEventListener('change', syncCompactPanels);
		void load().then(() => {
			openLocation(urlLocation(page.url.search));
			lastSearch = page.url.search;
			started = true;
		});
		const stop = chatApi.subscribe(organizationId, () => void load());
		const timer = setInterval(() => void load(), 30_000);
		return () => { stop(); clearInterval(timer); media.removeEventListener('change', syncCompactPanels); };
	});

	// A link followed while the screen is open (Open in Chat from another place).
	// Only when the address itself changes: a refresh of the data must not pull the owner back to a link they followed.
	$effect(() => {
		const search = page.url.search;
		const pending = pendingPlace;
		untrack(() => {
			if (!started || !chatState || search === lastSearch) return;
			if (pending && search !== pending.search) return;
			lastSearch = search;
			// Local navigation already applied the view and query. Back and deep links restore them.
			if (!pending) openLocation(urlLocation(search));
		});
	});

	function urlLocation(search: string) {
		const params = new URLSearchParams(search);
		return {
			...parseChatLocation(search),
			thread: params.get('thread') ?? undefined,
			view: params.get('v') ?? undefined,
			query: params.get('q') ?? ''
		};
	}
	function openLocation(loc: { conversation?: string; message?: string; thread?: string; view?: string; query?: string }) {
		if (!chatState) return;
		const found = locate(chatState, loc);
		if (!found.conversation) {
			const virtual = loc.view;
			const implicitPlace = !loc.conversation && !loc.message && virtual === undefined;
			const general = implicitPlace && bar?.needsYou.length === 0
				? chatState.conversations.find((c) => c.auto?.type === 'general') : undefined;
			if (general) select({ type: 'conversation', id: general.id }, false);
			else select({ type: virtual === 'mentions' || virtual === 'threads' ? virtual : 'needs' }, false);
			query = loc.query ?? '';
			return;
		}
		select({ type: 'conversation', id: found.conversation.id }, false);
		query = loc.query ?? '';
		if (found.thread) threadRoot = found.thread.id;
		else if (found.message && repliesTo(chatState, found.message.id).length) threadRoot = found.message.id;
		const linkedThread = loc.thread;
		if (linkedThread && chatState.messages.some((m) => m.id === linkedThread && m.conversation_id === found.conversation!.id && !m.thread_root_id)) threadRoot = linkedThread;
		highlight = found.message?.id;
		void tick().then(() => document.querySelector(`[data-id="${CSS.escape(found.message?.id ?? '')}"]`)?.scrollIntoView({ block: 'center' }));
	}

	function select(next: ChatView, navigate = true) {
		if (sidebarOpen) { sidebarReturn = 'selection'; sidebarOpen = false; }
		if (sidebarDialogMode) sidebarDialogSelected = true;
		query = '';
		view = next;
		threadRoot = undefined;
		threadOpener = undefined;
		showMembers = false;
		membersOpener = undefined;
		showMuted = false;
		openDigest = undefined;
		highlight = undefined;
		readMarker = next.type === 'conversation' ? chatState?.read_at[next.id] : undefined;
		if (next.type === 'conversation') void tick().then(() => listEl && (listEl.scrollTop = listEl.scrollHeight));
		if (navigate) writePlace();
	}

	// Use normal SvelteKit navigation so Back/Forward and direct links share one address model.
	function writePlace(messageId?: string) {
		const next = new URL(page.url);
		for (const key of ['c', 'm', 'thread', 'v', 'q']) next.searchParams.delete(key);
		if (view.type === 'conversation') next.searchParams.set('c', view.id);
		else next.searchParams.set('v', view.type);
		if (threadRoot) next.searchParams.set('thread', threadRoot);
		if (messageId) next.searchParams.set('m', messageId);
		if (query.trim()) next.searchParams.set('q', query);
		navigateToPlace(next);
	}

	type ChatHistoryState = App.PageState & { chatSearchEntry?: true };
	function changeQuery(value: string) {
		const current = page.url;
		const next = new URL(pendingPlace ?? current);
		const existingSearch = current.searchParams.has('q');
		const state: ChatHistoryState = page.state;
		if (value.trim()) {
			query = value;
			next.searchParams.set('q', value);
			const searchState: ChatHistoryState = existingSearch ? state : { ...state, chatSearchEntry: true };
			navigateToPlace(next, { replaceState: existingSearch, state: searchState });
			return;
		}
		// Return to the underlying entry if this tab opened Search. A direct Search link has no such entry.
		if (existingSearch && state.chatSearchEntry) {
			history.back();
			return;
		}
		query = value;
		next.searchParams.delete('q');
		navigateToPlace(next, { replaceState: existingSearch });
	}

	function navigateToPlace(next: URL, options: { replaceState?: boolean; state?: App.PageState } = {}) {
		if (next.href === (pendingPlace ?? page.url).href) return;
		pendingPlace = next;
		void goto(next, {
			...options,
			replaceState: options.replaceState || next.href === page.url.href,
			keepFocus: true,
			noScroll: true
		}).finally(() => {
			if (pendingPlace === next) pendingPlace = undefined;
		});
	}
	function closeThread(restore = true) {
		const wasOpen = !!threadRoot;
		threadRoot = undefined;
		highlight = undefined;
		writePlace();
		const target = threadOpener?.isConnected ? threadOpener : listEl;
		if (wasOpen && restore) void tick().then(() => focusVisible(target));
	}
	function closeMembers() {
		showMembers = false;
		const target = membersOpener?.isConnected ? membersOpener : listEl;
		void tick().then(() => focusVisible(target));
	}
	function toggleMembers(opener: HTMLElement) {
		if (showMembers) return closeMembers();
		membersOpener = opener;
		showMembers = true;
		closeThread(false);
	}
	// Opening a conversation reads it: what arrives while it is open is read too (unread muted messages don't count).
	$effect(() => {
		const c = conversation;
		const s = chatState;
		if (!c || !s) return;
		untrack(() => {
			if (unreadIn(s, ctx, c).unread > 0) void act({ method: 'chat.mark_read', conversation_id: c.id }, undefined, true);
		});
	});

	// New messages scroll the list to the bottom when it was there already.
	let lastCount = 0;
	$effect(() => {
		const n = (convView?.shown.length ?? 0) + pending.length;
		const c = conversation?.id;
		untrack(() => {
			if (listEl && c && n > lastCount && listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight < 200) void tick().then(() => listEl && (listEl.scrollTop = listEl.scrollHeight));
			lastCount = n;
		});
	});

	async function act(draft: Parameters<typeof send>[0], success?: string, quiet = false) {
		try {
			const accepted = await send(draft);
			await load();
			if (success) toast.success(success);
			return accepted;
		} catch (e) {
			if (!quiet) toast.error(toAppError(e).message);
			return undefined;
		}
	}

	// ---- sending -------------------------------------------------------------------------------

	interface Pending {
		key: number;
		/** Kept across a Retry, so a message the office already has is not posted twice. */
		change_id: string;
		conversation_id: string;
		thread_root_id?: string;
		also_in_channel?: boolean;
		body: string;
		attachments: string[];
		status: 'sending' | 'failed';
		error?: string;
	}
	let pending = $state<Pending[]>([]);
	let keys = 0;
	const pendingHere = $derived(pending.filter((p) => p.conversation_id === conversation?.id && (!p.thread_root_id || p.also_in_channel)));
	const pendingMessage = (p: Pending): ChatMessage => ({ id: `pending-${p.key}`, conversation_id: p.conversation_id, thread_root_id: p.thread_root_id, also_in_channel: p.also_in_channel, from: ctx.owner, kind: 'msg', body: p.body, created_at: now, attachments: p.attachments });

	const env = $derived<CommandEnv>({ projects: (snapshot.projects ?? []).map((p) => ({ id: p.id, name: p.name })), members: ctx.members, conversationTitle: conversation && chatState ? titleOf(conversation, ctx, ctx.owner) : 'this conversation' });
	let command = $state<CommandForm | undefined>();
	let commandOpen = $state(false);
	function openForm(form: CommandForm | { error: string }) {
		if ('error' in form) {
			toast.error(form.error);
			return false;
		}
		command = form;
		commandOpen = true;
		return true;
	}

	/** The composer's text: a slash command opens its form, anything else is sent (CHAT-9). */
	async function submit(text: string, options: { alsoInChannel: boolean; attachments: string[]; asMessage?: boolean }, c: Conversation, thread?: string): Promise<boolean> {
		const slash = options.asMessage ? undefined : parseSlash(text);
		if (slash) {
			if (!slash.ok) {
				toast.error(slash.message);
				return false;
			}
			return openForm(commandForm(slash, env));
		}
		pending.push({ key: ++keys, change_id: newId(), conversation_id: c.id, thread_root_id: thread, also_in_channel: thread && options.alsoInChannel ? true : undefined, body: text.trim(), attachments: options.attachments, status: 'sending' });
		// The reactive copy: changing it redraws the row.
		void deliver(pending[pending.length - 1]);
		return true;
	}
	async function deliver(p: Pending, retry = false, retryButton?: HTMLElement) {
		const origin = retry ? document.activeElement : null;
		const area = (retryButton ?? origin)?.closest('[data-focus-scope]');
		const place = focusPlace();
		let deliveredId: string | undefined;
		p.status = 'sending';
		try {
			const accepted = await chatApi.command(organizationId, { method: 'msg.send', change_id: p.change_id, conversation_id: p.conversation_id, thread_root_id: p.thread_root_id, also_in_channel: p.also_in_channel, kind: 'msg', body: p.body, attachments: p.attachments.length ? p.attachments : undefined } satisfies ChatCall);
			deliveredId = accepted.id;
			pending = pending.filter((x) => x.key !== p.key);
			await load();
		} catch (e) {
			p.status = 'failed';
			p.error = toAppError(e).message;
		}
		if (origin) await restoreFocus(origin, place, () => area?.querySelector<HTMLElement>(`[data-id="${CSS.escape(deliveredId ?? `pending-${p.key}`)}"]`) ?? null);
	}

	// ---- message actions -----------------------------------------------------------------------

	const react = (m: ChatMessage, reaction: Reaction) => { if (officeRunning) void act({ method: 'chat.react', message_id: m.id, reaction }); };
	const pin = (m: ChatMessage) => { if (officeRunning) void act({ method: 'chat.pin', message_id: m.id }); };
	const muteIn = (conversation_id: string, member_id: string, mode: MuteMode) => { if (officeRunning) void act({ method: 'chat.mute', conversation_id, member_id, mode }, mode === 'none' ? `${nameOf(ctx, member_id)} is back.` : mode === 'muted' ? `${nameOf(ctx, member_id)} is muted here. Their messages still reach Activity.` : `${nameOf(ctx, member_id)} is on a daily digest here.`); };
	const mute = (m: ChatMessage, mode: MuteMode) => muteIn(m.conversation_id, m.from, mode);
	const turn = (m: ChatMessage, into: 'bug' | 'request' | 'task') => openForm(turnForm(into, m, env));
	// Restore only focus lost by this update; never pull it from a control the owner moved to.
	const focusPlace = () => `${view.type}:${viewId ?? ''}:${threadRoot ?? ''}`;
	async function restoreFocus(origin: Element | null, place: string, target: () => HTMLElement | null) {
		await tick();
		if (!origin || place !== focusPlace() || (document.activeElement !== document.body && document.activeElement !== origin)) return;
		const next = target() ?? listEl;
		if (!next) return;
		// Generic rows become focusable only for this intentional return, so pointer clicks never acquire row focus.
		const temporary = next.tabIndex < 0 && !next.hasAttribute('tabindex');
		if (temporary) next.tabIndex = -1;
		next.focus({ preventScroll: true });
		if (temporary) {
			const clear = () => { if (next.getAttribute('tabindex') === '-1') next.removeAttribute('tabindex'); };
			if (document.activeElement === next) next.addEventListener('blur', clear, { once: true });
			else clear();
		}
	}
	async function answer(m: ChatMessage, a: { option_id?: string; text?: string }) {
		if (!officeRunning) throw new Error("The office isn't running. Restart it from Offices.");
		const origin = document.activeElement;
		const area = origin?.closest('[data-focus-scope]');
		const place = focusPlace();
		await send({ method: 'chat.answer', ask_id: m.id, ...a });
		await load();
		toast.success(`Answer sent to ${nameOf(ctx, m.from)}.`);
		await restoreFocus(origin, place, () => view.type === 'needs' && !searching
			? document.querySelector<HTMLElement>('[data-testid="needs-you-list"] [data-testid="ask-card"] button, [data-testid="nothing-needs-you"]')
			: area?.querySelector<HTMLElement>(`[data-ask-id="${CSS.escape(m.id)}"]`) ?? null);
	}
	async function accept(m: ChatMessage) {
		if (!officeRunning) throw new Error("The office isn't running. Restart it from Offices.");
		await send({ method: 'chat.accept_handoff', message_id: m.id });
		await load();
	}
	function openThread(m: ChatMessage, opener: HTMLElement) {
		if (chatState && (searching || view.type !== 'conversation' || view.id !== m.conversation_id)) {
			select({ type: 'conversation', id: m.conversation_id }, false);
		}
		threadOpener = opener;
		threadRoot = m.thread_root_id ?? m.id;
		showMembers = false;
		writePlace();
	}
	const openMessage = (m: ChatMessage) => {
		openLocation({ conversation: m.conversation_id, message: m.id });
		writePlace(m.id);
	};

	// ---- channels and DMs ----------------------------------------------------------------------

	let channelMode = $state<'create' | 'rename' | 'members'>('create');
	let channelOpen = $state(false);
	let dmOpen = $state(false);
	let channelDialog = $state<{ dismiss: () => void } | undefined>();
	let dmDialog = $state<{ dismiss: () => void } | undefined>();
	beforeNavigate((navigation) => {
		if (navigation.type !== 'popstate' || (!sidebarOpen && !channelOpen && !dmOpen && !commandOpen)) return;
		// Back dismisses the modal first; the next Back leaves the conversation.
		navigation.cancel();
		if (sidebarOpen) { sidebarReturn = 'toggle'; sidebarOpen = false; }
		if (channelOpen) channelDialog?.dismiss();
		if (dmOpen) dmDialog?.dismiss();
		commandOpen = false;
	});
	function openChannelDialog(mode: typeof channelMode) {
		if (!officeRunning) return;
		channelMode = mode;
		channelOpen = true;
	}
	function requireRunningOffice() {
		if (!officeRunning) throw new Error("The office isn't running. Restart it from Offices.");
	}

	async function channelSubmit(r: { name: string; members: string[] }) {
		requireRunningOffice();
		if (channelMode === 'create') {
			const accepted = await send({ method: 'channel.create', name: r.name, members: r.members });
			await load();
			if (accepted.conversation_id) select({ type: 'conversation', id: accepted.conversation_id });
		} else if (conversation && channelMode === 'rename') {
			await send({ method: 'channel.rename', id: conversation.id, name: r.name });
			await load();
		} else if (conversation) {
			for (const id of r.members.filter((x) => !conversation.members.includes(x))) await send({ method: 'channel.add_member', id: conversation.id, member_id: id });
			for (const id of conversation.members.filter((x) => !r.members.includes(x) && x !== ctx.owner)) await send({ method: 'channel.remove_member', id: conversation.id, member_id: id });
			await load();
		}
	}
	async function dmSubmit(ids: string[]) {
		requireRunningOffice();
		const accepted = await send({ method: 'dm.open', members: ids });
		await load();
		if (accepted.conversation_id) select({ type: 'conversation', id: accepted.conversation_id });
	}

	const dayChange = (list: ChatMessage[], i: number) => i === 0 || dayLabel(list[i].created_at, now) !== dayLabel(list[i - 1].created_at, now);
	const newAt = $derived(readMarker && convView ? convView.shown.find((m) => m.from !== ctx.owner && m.created_at > readMarker!)?.id : undefined);
</script>

<svelte:window onkeydown={(event) => {
	if (event.key === 'Escape' && !event.defaultPrevented && panelKind && !sidebarOpen && !channelOpen && !dmOpen && !commandOpen) {
		const field = event.target;
		const textField = field instanceof HTMLTextAreaElement || (field instanceof HTMLInputElement && /^(text|search|email|url|tel|password|number)$/.test(field.type));
		if (event.isComposing || (textField && field.value !== '')) return;
		event.preventDefault();
		if (panelKind === 'thread') closeThread(); else closeMembers();
	}
}} />

<svelte:head><title>{documentPlace} · Chat · Agentic Org</title></svelte:head>

{#snippet rowFor(m: ChatMessage, extra: { where?: string; open?: boolean } = {})}
	<MessageRow message={m} {ctx} chatState={chatState!} {now} readOnly={bridgePreview} {officeRunning} where={extra.where} onreact={react} onthread={openThread} onturn={turn} onmute={mute} onpin={pin} onanswer={answer} onaccept={accept} onopen={extra.open ? openMessage : undefined} />
{/snippet}

{#snippet undeliveredRows(items: Pending[], inThread = false)}
	{#each items as p (p.key)}
		<MessageRow message={pendingMessage(p)} {ctx} chatState={chatState!} {now} {inThread} {officeRunning} delivery={{ status: p.status, error: p.error }} onretry={(button) => void deliver(p, true, button)} ondismiss={() => (pending = pending.filter((x) => x.key !== p.key))} />
	{/each}
{/snippet}

<!-- At high zoom, keep a readable message area and let the shell scroll vertically. -->
<div bind:this={chatEl} onfocusin={revealChatFocus} class="chat-frame relative flex min-h-0 min-w-0">
	{#if loadError}
		<p class="m-8 text-sm/6 text-red-400" role="alert">{loadError}</p>
	{:else if !chatState || !bar}
		<p class="m-8 text-sm/6 text-gray-400" role="status">Loading chat…</p>
	{:else}
		<ChatSidebar bind:this={conversationNavigation} {bar} {view} {ctx} {officeRunning} container={chatEl} id={conversationsId} bind:inline={sidebarInline} bind:open={sidebarOpen} onCloseFocus={closeSidebarFocus} onselect={select} onnewchannel={(opener) => startSidebarDialog('channel', opener)} onnewmessage={(opener) => startSidebarDialog('dm', opener)} />

		<section bind:this={conversationEl} inert={(compactPanels && !!panelKind) || sidebarOpen} class="flex min-w-0 flex-1 flex-col overflow-y-auto" aria-label="Conversation">
			<div class="flex flex-wrap items-center justify-between gap-3 border-b border-white/10 px-5 py-3">
				<div class="min-w-0">
					<div class="flex min-w-0 items-center gap-2">
						{#if !sidebarInline}
							<Button bind:ref={sidebarToggle} variant="outline" size="sm" aria-haspopup="dialog" aria-expanded={sidebarOpen} aria-controls={conversationsId} aria-label={`Conversations${bar.needsYou.length ? `, ${bar.needsYou.length} ${bar.needsYou.length === 1 ? 'needs' : 'need'} you` : ''}`} onclick={() => { sidebarReturn = 'toggle'; sidebarOpen = true; }}>
								Conversations
								{#if bar.needsYou.length}<span class="rounded-full bg-amber-400 px-1.5 text-xs/5 font-semibold text-gray-900" aria-hidden="true">{bar.needsYou.length}</span>{/if}
							</Button>
						{/if}
						<h2 bind:this={heading} tabindex="-1" class="min-w-0 flex-1 truncate text-base/7 font-semibold text-white focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring" data-testid="conversation-title">
							{viewTitle}
						</h2>
					</div>
					{#if conversation && !searching}<p class="text-xs/5 text-gray-400">{subtitle}{#if conversation.archived}{' · Archived'}{/if}</p>{/if}
				</div>
				<div class="flex max-w-full flex-wrap items-center gap-2 max-[640px]:w-full">
					<Badge variant="secondary" data-testid="sample-data-note">Sample data</Badge>
					{#if conversation && !searching}
						<Button type="button" variant="outline" size="sm" aria-pressed={showMembers} onclick={(event) => toggleMembers(event.currentTarget)}><Users aria-hidden="true" /> Members</Button>
					{/if}
					<div class="relative min-w-0 max-w-full max-[640px]:w-full">
						<Search class="pointer-events-none absolute top-1/2 left-2 size-4 -translate-y-1/2 text-gray-400" aria-hidden="true" />
						<label class="sr-only" for="chat-search">Search chat</label>
						<input id="chat-search" type="search" value={query} oninput={(event) => changeQuery(event.currentTarget.value)} placeholder="Search · from:@ada in:#atlas has:ask" class="w-56 max-w-full rounded-md bg-gray-800 py-1.5 pr-3 pl-8 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 placeholder:text-gray-500 focus-visible:outline-2 focus-visible:outline-indigo-400 max-[640px]:w-full" data-testid="chat-search" />
					</div>
				</div>
			</div>

			{#if bridgePreview}
				<Callout data-testid="bridge-preview">
					{#snippet icon()}<Info class="size-4" aria-hidden="true" />{/snippet}
					<p>Previewing how a bridge (Slack, Teams or Discord) shows this: read-only here, and asks to the owner and approvals open in Agentic Org.</p>
				</Callout>
			{/if}
			{#if !officeRunning}
				<Callout data-testid="office-down">
					{#snippet icon()}<Unplug class="size-4" aria-hidden="true" />{/snippet}
					<p>Read-only from the last copy. The office isn't running.</p>
				</Callout>
			{/if}

			<div bind:this={listEl} data-focus-scope class="min-h-16 flex-1 overflow-y-auto px-3 py-3 max-[640px]:px-[8px]" data-testid="message-list" tabindex="-1" role="region" aria-label="Message area">
				{#if searching}
					<div class="px-2 pb-3">
						<p class="text-sm/6 text-gray-300" data-testid="search-count">{results.length} {results.length === 1 ? 'result' : 'results'}</p>
						<ul class="mt-1 flex flex-wrap items-center gap-1.5 text-xs/5 text-gray-400" aria-label="Search filters" data-testid="search-help">
							<li>Filter by</li>
							{#each ['from:@name', 'in:#channel', 'member:@name', 'has:ask', 'has:thread', 'has:attachment', 'kind:announcement', 'kind:handoff', 'before:2026-11-08', 'after:2026-11-01'] as f (f)}
								<li class="rounded bg-white/10 px-1.5 py-0.5 font-mono text-gray-200">{f}</li>
							{/each}
						</ul>
					</div>
					<div class="space-y-1">
						{#each results as m (m.id)}
							{@const c = chatState.conversations.find((x) => x.id === m.conversation_id)}
							{@render rowFor(m, { where: c ? titleOf(c, ctx, ctx.owner) : undefined, open: true })}
						{:else}
							<p class="px-2 text-sm/6 text-gray-400" data-testid="no-results">Nothing matches.</p>
						{/each}
					</div>
				{:else if view.type === 'needs'}
					<div class="mx-auto max-w-3xl space-y-3" data-testid="needs-you-list">
						{#each bar.needsYou as m (m.id)}
							{@const c = chatState.conversations.find((x) => x.id === m.conversation_id)}
							<div class="space-y-1">
								<AskCard message={m} {ctx} {now} readOnly={bridgePreview} {officeRunning} where={c ? titleOf(c, ctx, ctx.owner) : undefined} onanswer={(a) => answer(m, a)} onaccept={() => accept(m)} onreplyinthread={(opener) => openThread(m, opener)} />
								<button type="button" class="px-1 text-xs/5 font-semibold text-indigo-300 hover:text-indigo-200" onclick={() => openMessage(m)}>Open in conversation</button>
							</div>
						{:else}
							<p class="px-2 py-8 text-center text-sm/6 text-gray-400" data-testid="nothing-needs-you" tabindex="-1">Nothing needs you.</p>
						{/each}
					</div>
				{:else if view.type === 'mentions'}
					<div class="space-y-1">
						{#each bar.mentions as m (m.id)}
							{@const c = chatState.conversations.find((x) => x.id === m.conversation_id)}
							{@render rowFor(m, { where: c ? titleOf(c, ctx, ctx.owner) : undefined, open: true })}
						{:else}
							<p class="px-2 py-8 text-center text-sm/6 text-gray-400">Nobody has mentioned you.</p>
						{/each}
					</div>
				{:else if view.type === 'threads'}
					<ul class="mx-auto max-w-3xl space-y-2" data-testid="threads-list">
						{#each bar.threads as t (t.root.id)}
							<li>
								<button type="button" class="w-full rounded-lg border border-white/10 bg-white/[0.03] p-3 text-left hover:bg-white/5" onclick={(event) => openThread(t.root, event.currentTarget)}>
									<span class="block text-xs/5 text-gray-400">{titleOf(t.conversation, ctx, ctx.owner)} · {nameOf(ctx, t.root.from)}</span>
									<span class="mt-0.5 block truncate text-sm/6 text-white">{t.root.body.split('\n')[0]}</span>
									<span class="mt-1 block text-xs/5 text-indigo-300">{t.replies} {t.replies === 1 ? 'reply' : 'replies'}{#if t.new_replies}{' · ' + t.new_replies + ' new'}{/if}</span>
								</button>
							</li>
						{:else}
							<li class="px-2 py-8 text-center text-sm/6 text-gray-400">No threads with you in them.</li>
						{/each}
					</ul>
				{:else if conversation && convView}
					{#key conversation.id}
						<div role="log" aria-label="Messages" aria-live="polite" aria-relevant="additions">
							{#if !conversation.members.includes(ctx.owner)}
								<p class="mb-2 rounded-md bg-white/5 px-3 py-2 text-sm/6 text-gray-300" data-testid="reading-as-owner">You're reading as the owner. You can reply here.</p>
							{/if}
							{#each convView.shown as m, i (m.id)}
								{#if dayChange(convView.shown, i)}
									<div class="my-2 flex items-center gap-x-3 px-2 text-xs/5 text-gray-400" aria-live="off"><span class="h-px flex-1 bg-white/10"></span>{dayLabel(m.created_at, now)}<span class="h-px flex-1 bg-white/10"></span></div>
								{/if}
								{#if m.id === newAt}
									<div class="my-2 flex items-center gap-x-3 px-2 text-xs/5 font-semibold text-indigo-300" aria-live="off" data-testid="new-divider"><span class="h-px flex-1 bg-indigo-400/40"></span>New<span class="h-px flex-1 bg-indigo-400/40"></span></div>
								{/if}
								<div class={highlight === m.id ? 'rounded-md ring-2 ring-indigo-400' : ''} aria-live={channelRowLive(m)}>{@render rowFor(m)}</div>
							{:else}
								{#if conversation.auto?.type === 'general'}
									<div class="mx-auto my-8 max-w-md rounded-lg border border-white/10 bg-white/[0.03] p-5 text-center" data-testid="welcome-card">
										<h3 class="text-sm/6 font-semibold text-white">Welcome to #general</h3>
										<p class="mt-1 text-sm/6 text-gray-300">Agents and people talk here. Agents post questions and updates that need someone. Everything else stays in Activity.</p>
									</div>
								{:else}
									<p class="px-2 py-8 text-center text-sm/6 text-gray-400">No messages yet.</p>
								{/if}
							{/each}

							{#each convView.digests as g (g.member_id)}
								<div class="mt-2 rounded-md border border-white/10 bg-white/[0.03] px-3 py-2" data-testid="digest-row">
									<button type="button" class="text-sm/6 font-semibold text-indigo-300" aria-expanded={openDigest === g.member_id} onclick={() => (openDigest = openDigest === g.member_id ? undefined : g.member_id)}>Daily digest · {nameOf(ctx, g.member_id)} · {g.messages.length} {g.messages.length === 1 ? 'update' : 'updates'}</button>
									{#if openDigest === g.member_id}
										<div class="mt-1 space-y-1">{#each g.messages as m (m.id)}<div aria-live={channelRowLive(m)}><MessageRow message={m} {ctx} {chatState} {now} readOnly /></div>{/each}</div>
									{/if}
								</div>
							{/each}
							{#if convView.muted.length}
								<div class="mt-2 rounded-md border border-white/10 bg-white/[0.03] px-3 py-2" data-testid="muted-row">
									<button type="button" class="text-sm/6 font-semibold text-indigo-300" aria-expanded={showMuted} onclick={() => (showMuted = !showMuted)}>{convView.muted.length} muted {convView.muted.length === 1 ? 'message' : 'messages'} · {showMuted ? 'Hide' : 'Show'}</button>
									<span class="text-xs/5 text-gray-400"> They still reach Activity.</span>
									{#if showMuted}<div class="mt-1 space-y-1">{#each convView.muted as m (m.id)}<div aria-live={channelRowLive(m)}><MessageRow message={m} {ctx} {chatState} {now} readOnly /></div>{/each}</div>{/if}
								</div>
							{/if}
						</div>
					{/key}
					{@render undeliveredRows(pendingHere)}
				{/if}
			</div>

			{#if conversation && !searching}
				<div class="px-5 pb-1 text-xs/5 text-gray-400" aria-live="polite" data-testid="typing">{typing ?? ' '}</div>
				<Composer placeholder={bridgePreview ? 'Read-only in the preview' : `Message ${titleOf(conversation, ctx, ctx.owner)}`} canWrite={!!compose?.canWrite && !bridgePreview} note={compose?.note} {conversation} {ctx} resetKey="conversation:{conversation.id}" onsend={(t, o) => submit(t, o, conversation, undefined)} />
			{/if}
		</section>

		{#if root && threadConversation}
			<ThreadPanel {sidebarInline} bind:this={panel} {root} conversation={threadConversation} {ctx} {chatState} {now} {officeRunning} readOnly={bridgePreview} onclose={closeThread} onsend={(t, o) => submit(t, o, threadConversation, root.id)} onreact={react} onturn={turn} onmute={mute} onpin={pin} onanswer={answer} onaccept={accept}>
				{#snippet undelivered()}{@render undeliveredRows(pending.filter((p) => p.thread_root_id === root.id), true)}{/snippet}
			</ThreadPanel>
		{:else if showMembers && conversation}
			<MembersPanel {sidebarInline} bind:this={panel} {conversation} {ctx} {chatState} {officeRunning} projectName={followsWords} onclose={closeMembers} onmute={(id, mode) => muteIn(conversation.id, id, mode)} onrename={() => openChannelDialog('rename')} onaddpeople={() => openChannelDialog('members')} onremove={(id) => { if (officeRunning) void act({ method: 'channel.remove_member', id: conversation.id, member_id: id }); }} onarchive={() => { if (officeRunning) void act({ method: 'channel.archive', id: conversation.id }, `#${conversation.name} is archived.`); }} />
		{/if}
	{/if}
</div>

<ChannelDialog bind:this={channelDialog} returnFocus={sidebarDialogMode ? finishSidebarDialog : undefined} bind:open={channelOpen} mode={channelMode} members={ctx.members} initialName={channelMode === 'rename' ? (conversation?.name ?? '') : ''} initialMembers={channelMode === 'members' ? (conversation?.members ?? []) : []} onsubmit={channelSubmit} />
<NewMessageDialog bind:this={dmDialog} returnFocus={sidebarDialogMode ? finishSidebarDialog : undefined} bind:open={dmOpen} members={ctx.members} onsubmit={dmSubmit} />
<CommandDialog bind:open={commandOpen} form={command} ondone={(message) => toast.success(message)} />

<style>
	.chat-frame {
		--chat-header-height: 4rem;
		height: calc(100dvh - var(--chat-header-height));
	}
	@media (width < 640px) {
		.chat-frame { --chat-header-height: 3.5rem; }
	}
	@media (width < 640px), (height <= 320px) {
		.chat-frame { min-height: 28rem; }
		/* Include the sticky shell header when scrolling brings focus into view. */
		.chat-frame :global(:focus) {
			scroll-margin-block-start: var(--chat-header-height);
		}
	}
</style>
