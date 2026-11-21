<script lang="ts">
	import { onMount, tick, untrack } from 'svelte';
	import { page } from '$app/state';
	import { beforeNavigate, goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import Info from '@lucide/svelte/icons/info';
	import Unplug from '@lucide/svelte/icons/unplug';
	import Callout from '$lib/components/Callout.svelte';
	import { Button } from '$lib/components/ui/button';
	import { shellApi } from '$lib/modules/shell/api';
	import {
		ChatSidebar,
		NewMessageDialog,
		Composer,
		MessageRow,
		ThreadPanel,
		ChannelDialog,
		MembersPanel,
		CommandDialog,
		commandForm,
		turnForm,
		type CommandEnv,
		type CommandForm,
		browseChannels,
		chatContext,
		composerState,
		conversationView,
		locate,
		nameOf,
		messageRows,
		pinsIn,
		RowActivity,
		parseSearch,
		parseSlash,
		repliesTo,
		searchMessages,
		sidebar,
		titleOf,
		typingWords,
		unreadIn,
		type ChatCall,
		type ChatMessage,
		type ChatSnapshot,
		type ChatView,
		type Conversation,
		type MuteMode,
		type QuestionAnswer,
		type Reaction
	} from '$lib/modules/messaging';
	import { newId } from '$lib/modules/organization/commands';
	import { toAppError, type OrgSnapshot } from '$lib/proto';
	import { chatApi, sender } from '../../api';
	import {
		conversationSubtitle,
		copyFreshness,
		followsWords,
		isTextInputTarget,
		pendingToMessage,
		urlLocation
	} from '../../helpers';
	import type { ChatLocation, ChatPanelKind, PendingMessage } from '../../types';
	import BrowseChannelsDialog from '../browse-channels/BrowseChannelsDialog.svelte';
	import PinsPanel from '../pin-message/PinsPanel.svelte';
	import ConversationHeader from './ConversationHeader.svelte';
	import MessageFeed from './MessageFeed.svelte';

	/** Chat (chat-ux.md): channels, DMs and threads where agents and people talk. */
	let {
		organizationId,
		snapshot
	}: {
		organizationId: string;
		snapshot: OrgSnapshot;
	} = $props();

	const componentId = $props.id();
	const conversationsId = componentId + '-conversations';
	const send = $derived(sender(organizationId));

	let chat = $state<ChatSnapshot | undefined>();
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
	let threadRoot = $state<string | undefined>();
	let threadOpener: HTMLElement | undefined;
	const rowActivity = new RowActivity();
	const rowIsActive = (message: ChatMessage) => rowActivity.active(message.id);

	function menuChanged(message: ChatMessage, open: boolean, opener: HTMLElement) {
		if (open) rowActivity.hold(message.id, opener, 'menu');
		else {
			rowActivity.closeMenu(opener);
			void tick().then(() => rowActivity.focused(document.activeElement));
		}
	}

	let showMembers = $state(false);
	let membersOpener: HTMLElement | undefined;
	let showPins = $state(false);
	let pinsOpener: HTMLElement | undefined;
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
	let documentPlace = $state('Chat');
	let listEl = $state<HTMLElement | undefined>();
	let chatEl = $state<HTMLElement | undefined>();
	let heading = $state<HTMLHeadingElement | undefined>();
	let sidebarInline = $state(true);
	let sidebarOpen = $state(false);
	let sidebarToggle = $state<HTMLButtonElement | HTMLAnchorElement | null>(null);
	let conversationNavigation = $state<
		{ focusSelected: (preferred?: HTMLElement) => void } | undefined
	>();

	$effect.pre(() => {
		if (!sidebarInline) return;
		untrack(() => {
			const origin = sidebarToggle;
			if (!origin || document.activeElement !== origin) return;
			void tick().then(() => {
				if (
					sidebarInline &&
					(document.activeElement === origin || document.activeElement === document.body)
				)
					conversationNavigation?.focusSelected();
			});
		});
	});

	let sidebarReturn: 'toggle' | 'selection' | 'dialog' = 'toggle';
	let sidebarDialogMode = $state(false);
	let sidebarDialogSelected = false;
	let sidebarDialogOpener: HTMLElement | undefined;

	function revealFocus(target: HTMLElement) {
		const rect = target.getBoundingClientRect();
		const x = rect.x + rect.width / 2;
		const inset = Math.min(1, rect.height / 2);
		const scrollMargin = Number.parseFloat(getComputedStyle(target).scrollMarginBlockStart) || 0;
		const clippedByViewport = rect.top < scrollMargin || rect.bottom > innerHeight;
		const covered = ![rect.top + inset, rect.bottom - inset].every((y) =>
			target.contains(document.elementFromPoint(x, y))
		);
		if (clippedByViewport || covered) target.scrollIntoView({ block: 'center', inline: 'nearest' });
	}

	function focusVisible(target: HTMLElement | null | undefined) {
		if (!target?.isConnected) return;
		target.focus({ preventScroll: true });
		if (compactPanels) revealFocus(target);
	}

	function revealChatFocus(event: FocusEvent) {
		const target = event.target;
		rowActivity.focused(target);
		if (
			!(target instanceof HTMLElement) ||
			!(conversationEl?.contains(target) || target.closest('[data-chat-panel]'))
		)
			return;
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
		if (sidebarOpen) {
			sidebarReturn = 'dialog';
			sidebarOpen = false;
		}
		if (kind === 'channel') openChannelDialog('create');
		else dmOpen = true;
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
	const conversation = $derived(
		viewId && chatState ? chatState.conversations.find((c) => c.id === viewId) : undefined
	);
	const searching = $derived(query.trim() !== '' && page.url.searchParams.has('q'));
	const convView = $derived(
		conversation && chatState ? conversationView(chatState, ctx, conversation.id) : undefined
	);
	const pins = $derived(conversation && chatState ? pinsIn(chatState, conversation.id) : []);
	const root = $derived(
		threadRoot && chatState ? chatState.messages.find((m) => m.id === threadRoot) : undefined
	);
	const threadConversation = $derived(
		root && chatState
			? chatState.conversations.find((c) => c.id === root.conversation_id)
			: undefined
	);

	function channelRowLive(message: ChatMessage): 'off' | undefined {
		return root && message.thread_root_id === root.id ? 'off' : undefined;
	}

	const panelKind = $derived<ChatPanelKind | undefined>(
		root && threadConversation
			? 'thread'
			: showMembers && conversation
				? 'members'
				: showPins && conversation
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

	$effect(() => {
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

	async function retryLoad() {
		loadError = undefined;
		await load();
		if (chatState && !started) {
			openLocation(urlLocation(page.url.search));
			lastSearch = page.url.search;
			started = true;
		}
	}

	onMount(() => {
		const media = window.matchMedia('(width < 1280px)');
		const syncCompactPanels = () => (compactPanels = media.matches);
		syncCompactPanels();
		media.addEventListener('change', syncCompactPanels);
		void load().then(() => {
			if (chatState) {
				openLocation(urlLocation(page.url.search));
				lastSearch = page.url.search;
				started = true;
			}
			shellApi.navigation(organizationId).then(
				(n) => (inboxCount = n.inboxOpen ?? undefined),
				() => (inboxCount = undefined)
			);
		});
		const stop = chatApi.subscribe(organizationId, () => void load());
		const timer = setInterval(() => void load(), 30_000);
		return () => {
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
		const found = locate(chatState, loc);
		if (!found.conversation) {
			if (loc.view === 'needs') {
				void goto(resolve('/inbox'), { replaceState: true });
				return;
			}
			const general = chatState.conversations.find((c) => c.auto?.type === 'general');
			if (loc.view === 'mentions' || loc.view === 'threads' || !general)
				select({ type: loc.view === 'threads' ? 'threads' : 'mentions' }, false);
			else select({ type: 'conversation', id: general.id }, false);
			query = loc.query ?? '';
			return;
		}
		select({ type: 'conversation', id: found.conversation.id }, false);
		query = loc.query ?? '';
		if (found.thread) threadRoot = found.thread.id;
		else if (found.message && repliesTo(chatState, found.message.id).length)
			threadRoot = found.message.id;
		const linkedThread = loc.thread;
		if (
			linkedThread &&
			chatState.messages.some(
				(m) =>
					m.id === linkedThread && m.conversation_id === found.conversation!.id && !m.thread_root_id
			)
		)
			threadRoot = linkedThread;
		highlight = found.message?.id;
		void tick().then(() =>
			document
				.querySelector(`[data-id="${CSS.escape(found.message?.id ?? '')}"]`)
				?.scrollIntoView({ block: 'center' })
		);
	}

	function select(next: ChatView, navigate = true) {
		if (sidebarOpen) {
			sidebarReturn = 'selection';
			sidebarOpen = false;
		}
		if (sidebarDialogMode) sidebarDialogSelected = true;
		query = '';
		view = next;
		threadRoot = undefined;
		threadOpener = undefined;
		rowActivity.clear();
		showMembers = false;
		membersOpener = undefined;
		showPins = false;
		pinsOpener = undefined;
		showMuted = false;
		openDigest = undefined;
		highlight = undefined;
		readMarker = next.type === 'conversation' ? chatState?.read_at[next.id] : undefined;
		if (next.type === 'conversation')
			void tick().then(() => listEl && (listEl.scrollTop = listEl.scrollHeight));
		if (navigate) writePlace();
	}

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

	type ChatHistoryState = typeof page.state & { chatSearchEntry?: true };
	function changeQuery(value: string) {
		const current = page.url;
		const next = new URL(pendingPlace ?? current);
		const existingSearch = current.searchParams.has('q');
		const state: ChatHistoryState = page.state;
		if (value.trim()) {
			query = value;
			next.searchParams.set('q', value);
			const searchState: ChatHistoryState = existingSearch
				? state
				: { ...state, chatSearchEntry: true };
			navigateToPlace(next, { replaceState: existingSearch, state: searchState });
			return;
		}
		if (existingSearch && state.chatSearchEntry) {
			history.back();
			return;
		}
		query = value;
		next.searchParams.delete('q');
		navigateToPlace(next, { replaceState: existingSearch });
	}

	function navigateToPlace(
		next: URL,
		options: { replaceState?: boolean; state?: typeof page.state } = {}
	) {
		if (next.href === (pendingPlace ?? page.url).href) return;
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

	function closeThread(restore = true) {
		const wasOpen = !!threadRoot;
		const opener = threadOpener;
		rowActivity.returning(opener, 'thread');
		threadRoot = undefined;
		highlight = undefined;
		writePlace();
		const target = threadOpener?.isConnected ? threadOpener : listEl;
		if (wasOpen && restore)
			void tick().then(() => {
				if (threadRoot || threadOpener !== opener) {
					rowActivity.focused(opener);
					return;
				}
				focusVisible(target);
				rowActivity.focused(opener);
				threadOpener = undefined;
			});
		else {
			rowActivity.release(opener, 'thread');
			threadOpener = undefined;
		}
	}

	function closeMembers(restore = true) {
		showMembers = false;
		if (!restore) return;
		const target = membersOpener?.isConnected ? membersOpener : listEl;
		void tick().then(() => focusVisible(target));
	}

	function closePins(restore = true) {
		showPins = false;
		if (!restore) return;
		const target = pinsOpener?.isConnected ? pinsOpener : listEl;
		void tick().then(() => focusVisible(target));
	}

	function toggleMembers(opener: HTMLElement) {
		if (showMembers) return closeMembers();
		membersOpener = opener;
		showMembers = true;
		showPins = false;
		closeThread(false);
	}

	function togglePins(opener: HTMLElement) {
		if (showPins) return closePins();
		pinsOpener = opener;
		showPins = true;
		showMembers = false;
		closeThread(false);
	}

	function showPinnedMessage(m: ChatMessage) {
		if (compactPanels) showPins = false;
		if (view.type === 'conversation' && view.id === m.conversation_id) {
			highlight = m.id;
			void tick().then(() =>
				document
					.querySelector(`[data-id="${CSS.escape(m.id)}"]`)
					?.scrollIntoView({ block: 'center' })
			);
			return;
		}
		openMessage(m);
	}

	$effect(() => {
		const c = conversation;
		const s = chatState;
		if (!c || !s) return;
		untrack(() => {
			if (unreadIn(s, ctx, c).unread > 0)
				void act({ method: 'chat.mark_read', conversation_id: c.id }, undefined, true);
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

	let pending = $state<PendingMessage[]>([]);
	let keys = 0;
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
	let command = $state<CommandForm | undefined>();
	let commandOpen = $state(false);
	let commandOpener = $state<HTMLElement | undefined>();

	function returnCommandFocus() {
		const opener = commandOpener;
		rowActivity.returning(opener, 'dialog');
		void tick().then(() => {
			if (commandOpen || commandOpener !== opener) {
				rowActivity.focused(opener);
				return;
			}
			focusVisible(opener?.isConnected ? opener : listEl);
			rowActivity.focused(opener);
			commandOpener = undefined;
		});
	}

	function openForm(form: CommandForm | { error: string }) {
		if ('error' in form) {
			toast.error(form.error);
			return false;
		}
		command = form;
		commandOpen = true;
		return true;
	}

	async function submit(
		text: string,
		options: { alsoInChannel: boolean; attachments: string[]; asMessage?: boolean },
		c: Conversation,
		thread?: string
	): Promise<boolean> {
		const slash = options.asMessage ? undefined : parseSlash(text);
		if (slash) {
			if (!slash.ok) {
				toast.error(slash.message);
				return false;
			}
			return openForm(commandForm(slash, env));
		}
		pending.push({
			key: ++keys,
			change_id: newId(),
			conversation_id: c.id,
			thread_root_id: thread,
			also_in_channel: thread && options.alsoInChannel ? true : undefined,
			body: text.trim(),
			attachments: options.attachments,
			status: 'sending'
		});
		void deliver(pending[pending.length - 1]);
		return true;
	}

	async function deliver(p: PendingMessage, retry = false, retryButton?: HTMLElement) {
		const origin = retry ? document.activeElement : null;
		const area = (retryButton ?? origin)?.closest('[data-focus-scope]');
		const place = focusPlace();
		let deliveredId: string | undefined;
		p.status = 'sending';
		try {
			const accepted = await chatApi.command(organizationId, {
				method: 'msg.send',
				change_id: p.change_id,
				conversation_id: p.conversation_id,
				thread_root_id: p.thread_root_id,
				also_in_channel: p.also_in_channel,
				kind: 'msg',
				body: p.body,
				attachments: p.attachments.length ? p.attachments : undefined
			} satisfies ChatCall);
			deliveredId = accepted.id;
			pending = pending.filter((x) => x.key !== p.key);
			await load();
		} catch (e) {
			const error = toAppError(e);
			// No answer from the office is an unknown outcome, never a verified failure.
			if (error.code === 'unavailable') p.status = 'unknown';
			else {
				p.status = 'failed';
				p.error = error.message;
			}
		}
		if (origin)
			await restoreFocus(
				origin,
				place,
				() =>
					area?.querySelector<HTMLElement>(
						`[data-id="${CSS.escape(deliveredId ?? `pending-${p.key}`)}"]`
					) ?? null
			);
	}

	async function checkStatus(p: PendingMessage) {
		p.status = 'checking';
		try {
			const known = await chatApi.status(organizationId, p.change_id);
			if (known.state === 'applied') {
				pending = pending.filter((x) => x.key !== p.key);
				await load();
				toast.success('Delivered.');
			} else {
				p.status = 'failed';
				p.error = 'the office has no record of it. Retry sends it once.';
			}
		} catch {
			p.status = 'unknown';
		}
	}

	// ---- message actions -----------------------------------------------------------------------

	const react = (m: ChatMessage, reaction: Reaction) => {
		if (officeRunning) void act({ method: 'chat.react', message_id: m.id, reaction });
	};
	const pin = (m: ChatMessage) => {
		if (officeRunning) void act({ method: 'chat.pin', message_id: m.id });
	};
	const muteIn = (conversation_id: string, member_id: string, mode: MuteMode) => {
		if (officeRunning)
			void act(
				{ method: 'chat.mute', conversation_id, member_id, mode },
				mode === 'none'
					? `${nameOf(ctx, member_id)} is back.`
					: mode === 'muted'
						? `${nameOf(ctx, member_id)} is muted here. Their messages still reach Activity.`
						: `${nameOf(ctx, member_id)} is on a daily digest here.`
			);
	};
	const mute = (m: ChatMessage, mode: MuteMode) => muteIn(m.conversation_id, m.from, mode);
	const turn = (m: ChatMessage, into: 'bug' | 'request' | 'task', opener: HTMLElement) => {
		if (openForm(turnForm(into, m, env))) {
			rowActivity.hold(m.id, opener, 'dialog');
			commandOpener = opener;
		}
	};

	const focusPlace = () => `${view.type}:${viewId ?? ''}:${threadRoot ?? ''}`;
	async function restoreFocus(
		origin: Element | null,
		place: string,
		target: () => HTMLElement | null
	) {
		await tick();
		if (
			!origin ||
			place !== focusPlace() ||
			(document.activeElement !== document.body && document.activeElement !== origin)
		)
			return;
		const next = target() ?? listEl;
		if (!next) return;
		const temporary = next.tabIndex < 0 && !next.hasAttribute('tabindex');
		if (temporary) next.tabIndex = -1;
		next.focus({ preventScroll: true });
		if (temporary) {
			const clear = () => {
				if (next.getAttribute('tabindex') === '-1') next.removeAttribute('tabindex');
			};
			if (document.activeElement === next) next.addEventListener('blur', clear, { once: true });
			else clear();
		}
	}

	async function cardCall(draft: Parameters<typeof send>[0]) {
		if (!officeRunning) throw new Error("The office isn't running. Restart it from Offices.");
		try {
			await send(draft);
		} catch (e) {
			await load();
			throw toAppError(e);
		}
		await load();
	}

	async function answer(
		m: ChatMessage,
		a: { option_id?: string; text?: string; answers?: QuestionAnswer[] }
	) {
		const origin = document.activeElement;
		const area = origin?.closest('[data-focus-scope]');
		const place = focusPlace();
		await cardCall({ method: 'chat.answer', ask_id: m.id, ...a });
		toast.success(`Answer sent to ${nameOf(ctx, m.from)}.`);
		await restoreFocus(
			origin,
			place,
			() => area?.querySelector<HTMLElement>(`[data-ask-id="${CSS.escape(m.id)}"]`) ?? null
		);
	}

	const accept = (m: ChatMessage) => cardCall({ method: 'chat.accept_handoff', message_id: m.id });
	const handBack = async (m: ChatMessage, reason: string) => {
		await cardCall({ method: 'chat.hand_back', message_id: m.id, reason });
		toast.success(`Handed back to ${nameOf(ctx, m.from)}.`);
	};
	const withdraw = async (m: ChatMessage, reason: string) => {
		await cardCall({ method: 'chat.withdraw', ask_id: m.id, reason });
		toast.success('Ask withdrawn.');
	};

	function openThread(m: ChatMessage, opener: HTMLElement) {
		if (chatState && (searching || view.type !== 'conversation' || view.id !== m.conversation_id)) {
			select({ type: 'conversation', id: m.conversation_id }, false);
		}
		rowActivity.release(threadOpener, 'thread');
		threadOpener = opener;
		rowActivity.hold(m.id, opener, 'thread');
		threadRoot = m.thread_root_id ?? m.id;
		showMembers = false;
		showPins = false;
		writePlace();
	}

	const openMessage = (m: ChatMessage) => {
		openLocation({ conversation: m.conversation_id, message: m.id });
		writePlace(m.id);
	};

	// ---- channels, browse and DMs --------------------------------------------------------------

	let channelMode = $state<'create' | 'rename' | 'members'>('create');
	let channelOpen = $state(false);
	let dmOpen = $state(false);
	let browseOpen = $state(false);
	let channelDialog = $state<{ dismiss: () => void } | undefined>();
	let dmDialog = $state<{ dismiss: () => void } | undefined>();
	let browseDialog = $state<{ dismiss: () => void } | undefined>();
	let browseOpener: HTMLElement | undefined;

	const browseRows = $derived(
		chatState ? browseChannels(chatState, ctx, snapshot.projects ?? [], snapshot.teams) : []
	);

	function hasOpenOverlay() {
		return [sidebarOpen, channelOpen, dmOpen, commandOpen, browseOpen].some(Boolean);
	}

	beforeNavigate((navigation) => {
		if (navigation.type !== 'popstate' || !hasOpenOverlay()) return;
		navigation.cancel();
		if (sidebarOpen) {
			sidebarReturn = 'toggle';
			sidebarOpen = false;
		}
		if (browseOpen) browseDialog?.dismiss();
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
			return;
		}
		if (!conversation) return;
		if (channelMode === 'rename') {
			await send({ method: 'channel.rename', id: conversation.id, name: r.name });
			await load();
			return;
		}
		for (const id of r.members.filter((x) => !conversation.members.includes(x)))
			await send({ method: 'channel.add_member', id: conversation.id, member_id: id });
		for (const id of conversation.members.filter((x) => !r.members.includes(x) && x !== ctx.owner))
			await send({ method: 'channel.remove_member', id: conversation.id, member_id: id });
		await load();
	}

	function openBrowse(opener: HTMLElement) {
		browseOpener = opener;
		if (sidebarOpen) {
			sidebarReturn = 'dialog';
			sidebarOpen = false;
		}
		browseOpen = true;
	}

	function browseReturn() {
		const opener = browseOpener;
		browseOpener = undefined;
		void tick().then(() => {
			if (document.activeElement !== document.body) return;
			if (opener?.isConnected) opener.focus();
			else if (sidebarInline) conversationNavigation?.focusSelected();
			else focusVisible(sidebarToggle);
		});
	}

	function browseShow(id: string) {
		browseOpener = undefined;
		browseOpen = false;
		select({ type: 'conversation', id });
		void tick().then(() => focusVisible(heading));
	}

	async function browseJoin(id: string) {
		requireRunningOffice();
		await send({ method: 'channel.join', id });
		await load();
		const c = chatState?.conversations.find((x) => x.id === id);
		toast.success(`You joined #${c?.name ?? 'the channel'}.`);
		browseShow(id);
	}

	async function browseRequest(id: string) {
		requireRunningOffice();
		await send({ method: 'channel.request_invitation', id });
		await load();
		toast.success('Invitation requested. A member of the channel can add you.');
	}

	async function dmSubmit(ids: string[]) {
		requireRunningOffice();
		const accepted = await send({ method: 'dm.open', members: ids });
		await load();
		if (accepted.conversation_id) select({ type: 'conversation', id: accepted.conversation_id });
	}

	function shouldClosePanelOnEscape(event: KeyboardEvent) {
		if (event.key !== 'Escape' || event.defaultPrevented || event.isComposing) return false;
		if (!panelKind || hasOpenOverlay()) return false;
		return !isTextInputTarget(event.target);
	}

	function handleWindowKeydown(event: KeyboardEvent) {
		if (!shouldClosePanelOnEscape(event)) return;
		event.preventDefault();
		if (panelKind === 'thread') closeThread();
		else if (panelKind === 'members') closeMembers();
		else closePins();
	}

	const newAt = $derived(
		readMarker && convView
			? convView.shown.find((m) => m.from !== ctx.owner && m.created_at > readMarker!)?.id
			: undefined
	);
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<svelte:head><title>{documentPlace} · Chat · Agentic Org</title></svelte:head>

{#snippet rowFor(m: ChatMessage, extra: { where?: string; open?: boolean } = {})}
	<MessageRow
		message={m}
		{ctx}
		rowData={rows!.forMessage(m)}
		{now}
		readOnly={bridgePreview}
		{officeRunning}
		active={rowIsActive(m)}
		onmenu={menuChanged}
		where={extra.where}
		onreact={react}
		onthread={openThread}
		onturn={turn}
		onmute={mute}
		onpin={pin}
		onanswer={answer}
		onaccept={accept}
		onhandback={handBack}
		onwithdraw={withdraw}
		onopen={extra.open ? openMessage : undefined}
	/>
{/snippet}

{#snippet undeliveredRows(items: PendingMessage[], inThread = false)}
	{#each items as p (p.key)}
		{@const message = pendingToMessage(p, ctx.owner, now)}
		<MessageRow
			{message}
			{ctx}
			rowData={rows!.forMessage(message)}
			{now}
			{inThread}
			{officeRunning}
			delivery={{ status: p.status, error: p.error }}
			oncheck={() => void checkStatus(p)}
			onretry={(button) => void deliver(p, true, button)}
			ondismiss={() => (pending = pending.filter((x) => x.key !== p.key))}
		/>
	{/each}
{/snippet}

<!-- At high zoom, keep a readable message area and let the shell scroll vertically. -->
<div
	bind:this={chatEl}
	onfocusin={revealChatFocus}
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
				onclick={() => void retryLoad()}>Retry</Button
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
			bind:inline={sidebarInline}
			bind:open={sidebarOpen}
			onCloseFocus={closeSidebarFocus}
			onselect={select}
			onnewchannel={(opener) => startSidebarDialog('channel', opener)}
			onnewmessage={(opener) => startSidebarDialog('dm', opener)}
			onbrowse={openBrowse}
		/>

		<section
			bind:this={conversationEl}
			inert={(compactPanels && !!panelKind) || sidebarOpen}
			class="flex min-w-0 flex-1 flex-col overflow-y-auto"
			aria-label="Conversation"
		>
			<ConversationHeader
				{viewTitle}
				{subtitle}
				{conversation}
				{searching}
				sample={!!chat?.sample}
				{sidebarInline}
				{sidebarOpen}
				{conversationsId}
				{inboxCount}
				pinCount={pins.length}
				{showPins}
				{showMembers}
				{query}
				bind:heading
				bind:sidebarToggle
				onopensidebar={() => {
					sidebarReturn = 'toggle';
					sidebarOpen = true;
				}}
				ontogglepins={togglePins}
				ontogglemembers={toggleMembers}
				onquerychange={changeQuery}
			/>

			{#if bridgePreview}
				<Callout data-testid="bridge-preview">
					{#snippet icon()}<Info class="size-4" aria-hidden="true" />{/snippet}
					<p>
						Previewing how a bridge (Slack, Teams or Discord) shows this: read-only here, and asks
						to the owner and approvals open in Agentic Org.
					</p>
				</Callout>
			{/if}
			{#if !officeRunning}
				<Callout data-testid="office-down">
					{#snippet icon()}<Unplug class="size-4" aria-hidden="true" />{/snippet}
					<p>
						Read-only from the last copy. The office isn't running.{copyAge ? ` ${copyAge}` : ''}
					</p>
				</Callout>
			{/if}

			<MessageFeed
				bind:listEl
				{searching}
				{results}
				{view}
				{bar}
				{conversation}
				{convView}
				{chatState}
				{ctx}
				{rows}
				{now}
				{newAt}
				{highlight}
				bind:showMuted
				bind:openDigest
				{channelRowLive}
				{rowFor}
				onopenthread={openThread}
			>
				{#snippet undelivered()}{@render undeliveredRows(pendingHere)}{/snippet}
			</MessageFeed>

			{#if conversation && !searching}
				<div class="px-5 pb-1 text-xs/5 text-fg-muted" aria-live="polite" data-testid="typing">
					{typing ?? ' '}
				</div>
				<Composer
					placeholder={bridgePreview
						? 'Read-only in the preview'
						: `Message ${titleOf(conversation, ctx, ctx.owner)}`}
					canWrite={!!compose?.canWrite && !bridgePreview}
					note={compose?.note}
					{cannotRun}
					{conversation}
					{ctx}
					resetKey="conversation:{conversation.id}"
					onsend={(t, o) => submit(t, o, conversation, undefined)}
				/>
			{/if}
		</section>

		{#if root && threadConversation}
			<ThreadPanel
				active={rowIsActive}
				onmenu={menuChanged}
				{rows}
				{sidebarInline}
				bind:this={panel}
				{root}
				conversation={threadConversation}
				{ctx}
				{chatState}
				{now}
				{officeRunning}
				{cannotRun}
				readOnly={bridgePreview}
				onclose={closeThread}
				onsend={(t, o) => submit(t, o, threadConversation, root.id)}
				onreact={react}
				onturn={turn}
				onmute={mute}
				onpin={pin}
				onanswer={answer}
				onaccept={accept}
				onhandback={handBack}
				onwithdraw={withdraw}
			>
				{#snippet undelivered()}{@render undeliveredRows(
						pending.filter((p) => p.thread_root_id === root.id),
						true
					)}{/snippet}
			</ThreadPanel>
		{:else if showMembers && conversation}
			<MembersPanel
				{sidebarInline}
				bind:this={panel}
				{conversation}
				{ctx}
				{chatState}
				{officeRunning}
				projectName={(c) => followsWords(c, snapshot)}
				onclose={closeMembers}
				onmute={(id, mode) => muteIn(conversation.id, id, mode)}
				onrename={() => openChannelDialog('rename')}
				onaddpeople={() => openChannelDialog('members')}
				onremove={(id) => {
					if (officeRunning)
						void act({ method: 'channel.remove_member', id: conversation.id, member_id: id });
				}}
				onarchive={() => {
					if (officeRunning)
						void act(
							{ method: 'channel.archive', id: conversation.id },
							`#${conversation.name} is archived.`
						);
				}}
			/>
		{:else if showPins && conversation}
			<PinsPanel
				{sidebarInline}
				bind:this={panel}
				{conversation}
				{ctx}
				{pins}
				{now}
				{officeRunning}
				onclose={closePins}
				onshow={showPinnedMessage}
				onunpin={pin}
			/>
		{/if}
	{/if}
</div>

<ChannelDialog
	bind:this={channelDialog}
	returnFocus={sidebarDialogMode ? finishSidebarDialog : undefined}
	bind:open={channelOpen}
	mode={channelMode}
	members={ctx.members}
	initialName={channelMode === 'rename' ? (conversation?.name ?? '') : ''}
	initialMembers={channelMode === 'members' ? (conversation?.members ?? []) : []}
	onsubmit={channelSubmit}
/>
<NewMessageDialog
	bind:this={dmDialog}
	returnFocus={sidebarDialogMode ? finishSidebarDialog : undefined}
	bind:open={dmOpen}
	members={ctx.members}
	onsubmit={dmSubmit}
/>
<BrowseChannelsDialog
	bind:this={browseDialog}
	bind:open={browseOpen}
	rows={browseRows}
	{officeRunning}
	returnFocus={browseReturn}
	onopen={browseShow}
	onjoin={browseJoin}
	onrequest={browseRequest}
/>
<CommandDialog
	returnFocus={commandOpener ? returnCommandFocus : undefined}
	bind:open={commandOpen}
	form={command}
	ondone={(message) => toast.success(message)}
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
