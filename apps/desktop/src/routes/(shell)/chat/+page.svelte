<script lang="ts">
	import { onMount, tick, untrack } from 'svelte';
	import { page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import Search from '@lucide/svelte/icons/search';
	import Users from '@lucide/svelte/icons/users';
	import AskCard from '$lib/components/chat/AskCard.svelte';
	import ChannelDialog from '$lib/components/chat/ChannelDialog.svelte';
	import ChatSidebar from '$lib/components/chat/ChatSidebar.svelte';
	import CommandDialog from '$lib/components/chat/CommandDialog.svelte';
	import Composer from '$lib/components/chat/Composer.svelte';
	import MembersPanel from '$lib/components/chat/MembersPanel.svelte';
	import MessageRow from '$lib/components/chat/MessageRow.svelte';
	import NewMessageDialog from '$lib/components/chat/NewMessageDialog.svelte';
	import ThreadPanel from '$lib/components/chat/ThreadPanel.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { chatApi, sender } from '$lib/chat';
	import { commandForm, turnForm, type CommandEnv, type CommandForm } from '$lib/chat/commands';
	import {
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
	} from '$lib/chat/model';
	import type { ChatCall, ChatMessage, ChatSnapshot, Conversation, MuteMode, Reaction } from '$lib/chat/types';
	import { newId } from '$lib/org/commands';
	import { toAppError } from '$lib/office/api';

	/** Chat (docs/specs/chat-ux.md): channels, DMs and threads where agents and people talk. A mock office; always sample data. */
	let { data } = $props();
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
	let showMembers = $state(false);
	let query = $state('');
	let readMarker = $state<string | undefined>();
	let showMuted = $state(false);
	let openDigest = $state<string | undefined>();
	let highlight = $state<string | undefined>();
	let started = false;
	let lastSearch = '';
	let listEl = $state<HTMLElement | undefined>();

	// Previews how a bridge shows asks (CHAT-17): a mock hook like `?mock=`, `?org=` and `?chat=`.
	const bridgePreview = $derived(page.url.searchParams.get('surface') === 'bridge');

	const viewId = $derived(view.type === 'conversation' ? view.id : undefined);
	const conversation = $derived(viewId && chatState ? chatState.conversations.find((c) => c.id === viewId) : undefined);
	const searching = $derived(query.trim() !== '');
	const convView = $derived(conversation && chatState ? conversationView(chatState, ctx, conversation.id) : undefined);
	const root = $derived(threadRoot && chatState ? chatState.messages.find((m) => m.id === threadRoot) : undefined);
	const threadConversation = $derived(root && chatState ? chatState.conversations.find((c) => c.id === root.conversation_id) : undefined);
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
		void load().then(() => {
			// A link from Activity opens the message; otherwise the owner starts where something waits for them.
			const loc = parseChatLocation(page.url.search);
			if (chatState && (loc.conversation || loc.message)) openLocation(loc);
			else if (bar && bar.needsYou.length === 0) select({ type: 'conversation', id: chatState!.conversations.find((c) => c.auto?.type === 'general')?.id ?? '' });
			lastSearch = page.url.search;
			started = true;
		});
		const stop = chatApi.subscribe(organizationId, () => void load());
		const timer = setInterval(() => void load(), 30_000);
		return () => (stop(), clearInterval(timer));
	});

	// A link followed while the screen is open (Open in Chat from another place).
	// Only when the address itself changes: a refresh of the data must not pull the owner back to a link they followed.
	$effect(() => {
		const search = page.url.search;
		untrack(() => {
			if (!started || !chatState || search === lastSearch) return;
			lastSearch = search;
			openLocation(parseChatLocation(search));
		});
	});

	function openLocation(loc: { conversation?: string; message?: string }) {
		if (!chatState) return;
		const found = locate(chatState, loc);
		if (!found.conversation) return;
		select({ type: 'conversation', id: found.conversation.id });
		if (found.thread) threadRoot = found.thread.id;
		else if (found.message && repliesTo(chatState, found.message.id).length) threadRoot = found.message.id;
		highlight = found.message?.id;
		void tick().then(() => document.querySelector(`[data-id="${CSS.escape(found.message?.id ?? '')}"]`)?.scrollIntoView({ block: 'center' }));
	}

	function select(next: ChatView) {
		query = '';
		view = next;
		threadRoot = undefined;
		showMembers = false;
		showMuted = false;
		openDigest = undefined;
		highlight = undefined;
		readMarker = next.type === 'conversation' ? chatState?.read_at[next.id] : undefined;
		if (next.type === 'conversation') void tick().then(() => listEl && (listEl.scrollTop = listEl.scrollHeight));
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
		body: string;
		attachments: string[];
		status: 'sending' | 'failed';
		error?: string;
	}
	let pending = $state<Pending[]>([]);
	let keys = 0;
	const pendingHere = $derived(pending.filter((p) => p.conversation_id === conversation?.id));
	const pendingMessage = (p: Pending): ChatMessage => ({ id: `pending-${p.key}`, conversation_id: p.conversation_id, from: ctx.owner, kind: 'msg', body: p.body, created_at: now, attachments: p.attachments });

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
	async function submit(text: string, options: { alsoInChannel: boolean; attachments: string[] }, c: Conversation, thread?: string): Promise<boolean> {
		const slash = parseSlash(text);
		if (slash) {
			if (!slash.ok) {
				toast.error(slash.message);
				return false;
			}
			return openForm(commandForm(slash, env));
		}
		if (thread) {
			const accepted = await act({ method: 'msg.send', conversation_id: c.id, thread_root_id: thread, kind: 'msg', body: text, also_in_channel: options.alsoInChannel || undefined, attachments: options.attachments.length ? options.attachments : undefined });
			return !!accepted;
		}
		pending.push({ key: ++keys, change_id: newId(), conversation_id: c.id, body: text, attachments: options.attachments, status: 'sending' });
		// The reactive copy: changing it redraws the row.
		void deliver(pending[pending.length - 1]);
		return true;
	}
	async function deliver(p: Pending) {
		p.status = 'sending';
		try {
			await chatApi.command(organizationId, { method: 'msg.send', change_id: p.change_id, conversation_id: p.conversation_id, kind: 'msg', body: p.body, attachments: p.attachments.length ? p.attachments : undefined } satisfies ChatCall);
			pending = pending.filter((x) => x.key !== p.key);
			await load();
		} catch (e) {
			p.status = 'failed';
			p.error = toAppError(e).message;
		}
	}

	// ---- message actions -----------------------------------------------------------------------

	const react = (m: ChatMessage, reaction: Reaction) => void act({ method: 'chat.react', message_id: m.id, reaction });
	const pin = (m: ChatMessage) => void act({ method: 'chat.pin', message_id: m.id });
	const muteIn = (conversation_id: string, member_id: string, mode: MuteMode) => void act({ method: 'chat.mute', conversation_id, member_id, mode }, mode === 'none' ? `${nameOf(ctx, member_id)} is back.` : mode === 'muted' ? `${nameOf(ctx, member_id)} is muted here. Their messages still reach Activity.` : `${nameOf(ctx, member_id)} is on a daily digest here.`);
	const mute = (m: ChatMessage, mode: MuteMode) => muteIn(m.conversation_id, m.from, mode);
	const turn = (m: ChatMessage, into: 'bug' | 'request' | 'task') => openForm(turnForm(into, m, env));
	async function answer(m: ChatMessage, a: { option_id?: string; text?: string }) {
		await send({ method: 'chat.answer', ask_id: m.id, ...a });
		await load();
		toast.success(`Answer sent to ${nameOf(ctx, m.from)}.`);
	}
	async function accept(m: ChatMessage) {
		await send({ method: 'chat.accept_handoff', message_id: m.id });
		await load();
	}
	function openThread(m: ChatMessage) {
		if (chatState && view.type !== 'conversation') select({ type: 'conversation', id: m.conversation_id });
		else if (chatState && view.type === 'conversation' && view.id !== m.conversation_id) select({ type: 'conversation', id: m.conversation_id });
		threadRoot = m.thread_root_id ?? m.id;
		showMembers = false;
	}
	const openMessage = (m: ChatMessage) => openLocation({ conversation: m.conversation_id, message: m.id });

	// ---- channels and DMs ----------------------------------------------------------------------

	let channelMode = $state<'create' | 'rename' | 'members'>('create');
	let channelOpen = $state(false);
	let dmOpen = $state(false);
	const openChannelDialog = (mode: typeof channelMode) => ((channelMode = mode), (channelOpen = true));

	async function channelSubmit(r: { name: string; members: string[] }) {
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
		const accepted = await send({ method: 'dm.open', members: ids });
		await load();
		if (accepted.conversation_id) select({ type: 'conversation', id: accepted.conversation_id });
	}

	const dayChange = (list: ChatMessage[], i: number) => i === 0 || dayLabel(list[i].created_at, now) !== dayLabel(list[i - 1].created_at, now);
	const newAt = $derived(readMarker && convView ? convView.shown.find((m) => m.from !== ctx.owner && m.created_at > readMarker!)?.id : undefined);
</script>

<svelte:head><title>Chat · Agentic Org</title></svelte:head>

{#snippet rowFor(m: ChatMessage, extra: { where?: string; open?: boolean } = {})}
	<MessageRow message={m} {ctx} chatState={chatState!} {now} readOnly={bridgePreview} where={extra.where} onreact={react} onthread={openThread} onturn={turn} onmute={mute} onpin={pin} onanswer={answer} onaccept={accept} onopen={extra.open ? openMessage : undefined} />
{/snippet}

<div class="flex h-[calc(100dvh-4rem)] min-h-0">
	{#if loadError}
		<p class="m-8 text-sm/6 text-red-400" role="alert">{loadError}</p>
	{:else if !chatState || !bar}
		<p class="m-8 text-sm/6 text-gray-400" role="status">Loading chat…</p>
	{:else}
		<ChatSidebar {bar} {view} {ctx} onselect={select} onnewchannel={() => openChannelDialog('create')} onnewmessage={() => (dmOpen = true)} />

		<section class="flex min-w-0 flex-1 flex-col" aria-label="Conversation">
			<div class="flex flex-wrap items-center justify-between gap-3 border-b border-white/10 px-5 py-3">
				<div class="min-w-0">
					<h2 class="truncate text-base/7 font-semibold text-white" data-testid="conversation-title">
						{#if searching}Search{:else if view.type === 'needs'}Needs you{:else if view.type === 'mentions'}Mentions{:else if view.type === 'threads'}Threads{:else if conversation}{titleOf(conversation, ctx, ctx.owner)}{/if}
					</h2>
					{#if conversation && !searching}<p class="text-xs/5 text-gray-400">{subtitle}{#if conversation.archived}{' · Archived'}{/if}</p>{/if}
				</div>
				<div class="flex flex-wrap items-center gap-2">
					<Badge variant="secondary" data-testid="sample-data-note">Sample data</Badge>
					{#if conversation && !searching}
						<Button type="button" variant="outline" size="sm" aria-pressed={showMembers} onclick={() => ((showMembers = !showMembers), (threadRoot = undefined))}><Users aria-hidden="true" /> Members</Button>
					{/if}
					<div class="relative">
						<Search class="pointer-events-none absolute top-1/2 left-2 size-4 -translate-y-1/2 text-gray-400" aria-hidden="true" />
						<label class="sr-only" for="chat-search">Search chat</label>
						<input id="chat-search" type="search" bind:value={query} placeholder="Search · from:@ada in:#atlas has:ask" class="w-56 rounded-md bg-gray-800 py-1.5 pr-3 pl-8 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 placeholder:text-gray-500 focus-visible:outline-2 focus-visible:outline-indigo-400" data-testid="chat-search" />
					</div>
				</div>
			</div>

			{#if bridgePreview}
				<p class="border-b border-white/10 bg-sky-400/10 px-5 py-2 text-sm/6 text-sky-100" data-testid="bridge-preview">Previewing how a bridge (Slack, Teams or Discord) shows this: read-only here, and asks to the owner and approvals open in Agentic Org.</p>
			{/if}
			{#if !officeRunning}
				<p class="border-b border-white/10 bg-amber-400/10 px-5 py-2 text-sm/6 text-amber-100" data-testid="office-down">Read-only from the last copy. The office isn't running.</p>
			{/if}

			<div bind:this={listEl} class="min-h-0 flex-1 overflow-y-auto px-3 py-3" data-testid="message-list">
				{#if searching}
					<p class="px-2 pb-2 text-sm/6 text-gray-400" data-testid="search-count">{results.length} {results.length === 1 ? 'result' : 'results'}. Filters: <code>from:@name in:#channel member:@name has:ask|thread|attachment kind:announcement|handoff before:2026-11-08 after:2026-11-01</code></p>
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
								<AskCard message={m} {ctx} {now} readOnly={bridgePreview} where={c ? titleOf(c, ctx, ctx.owner) : undefined} onanswer={(a) => answer(m, a)} onaccept={() => accept(m)} onreplyinthread={() => openThread(m)} />
								<button type="button" class="px-1 text-xs/5 font-semibold text-indigo-300 hover:text-indigo-200" onclick={() => openMessage(m)}>Open in conversation</button>
							</div>
						{:else}
							<p class="px-2 py-8 text-center text-sm/6 text-gray-400" data-testid="nothing-needs-you">Nothing needs you.</p>
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
								<button type="button" class="w-full rounded-lg border border-white/10 bg-white/[0.03] p-3 text-left hover:bg-white/5" onclick={() => openThread(t.root)}>
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
					{#if !conversation.members.includes(ctx.owner)}
						<p class="mb-2 rounded-md bg-white/5 px-3 py-2 text-sm/6 text-gray-300" data-testid="reading-as-owner">You're reading as the owner. You can reply here.</p>
					{/if}
					{#each convView.shown as m, i (m.id)}
						{#if dayChange(convView.shown, i)}
							<div class="my-2 flex items-center gap-x-3 px-2 text-xs/5 text-gray-400"><span class="h-px flex-1 bg-white/10"></span>{dayLabel(m.created_at, now)}<span class="h-px flex-1 bg-white/10"></span></div>
						{/if}
						{#if m.id === newAt}
							<div class="my-2 flex items-center gap-x-3 px-2 text-xs/5 font-semibold text-indigo-300" data-testid="new-divider"><span class="h-px flex-1 bg-indigo-400/40"></span>New<span class="h-px flex-1 bg-indigo-400/40"></span></div>
						{/if}
						<div class={highlight === m.id ? 'rounded-md ring-2 ring-indigo-400' : ''}>{@render rowFor(m)}</div>
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
								<div class="mt-1 space-y-1">{#each g.messages as m (m.id)}<MessageRow message={m} {ctx} {chatState} {now} readOnly />{/each}</div>
							{/if}
						</div>
					{/each}
					{#if convView.muted.length}
						<div class="mt-2 rounded-md border border-white/10 bg-white/[0.03] px-3 py-2" data-testid="muted-row">
							<button type="button" class="text-sm/6 font-semibold text-indigo-300" aria-expanded={showMuted} onclick={() => (showMuted = !showMuted)}>{convView.muted.length} muted {convView.muted.length === 1 ? 'message' : 'messages'} · {showMuted ? 'Hide' : 'Show'}</button>
							<span class="text-xs/5 text-gray-400"> They still reach Activity.</span>
							{#if showMuted}<div class="mt-1 space-y-1">{#each convView.muted as m (m.id)}<MessageRow message={m} {ctx} {chatState} {now} readOnly />{/each}</div>{/if}
						</div>
					{/if}
					{#each pendingHere as p (p.key)}
						<MessageRow message={pendingMessage(p)} {ctx} {chatState} {now} delivery={{ status: p.status, error: p.error }} onretry={() => void deliver(p)} ondismiss={() => (pending = pending.filter((x) => x.key !== p.key))} />
					{/each}
				{/if}
			</div>

			{#if conversation && !searching}
				<div class="px-5 pb-1 text-xs/5 text-gray-400" aria-live="polite" data-testid="typing">{typing ?? ' '}</div>
				<Composer placeholder={bridgePreview ? 'Read-only in the preview' : `Message ${titleOf(conversation, ctx, ctx.owner)}`} canWrite={!!compose?.canWrite && !bridgePreview} note={compose?.note} {conversation} {ctx} resetKey="conversation:{conversation.id}" onsend={(t, o) => submit(t, o, conversation, undefined)} />
			{/if}
		</section>

		{#if root && threadConversation}
			<ThreadPanel {root} conversation={threadConversation} {ctx} {chatState} {now} {officeRunning} readOnly={bridgePreview} onclose={() => (threadRoot = undefined)} onsend={(t, o) => submit(t, o, threadConversation, root.id)} onreact={react} onturn={turn} onmute={mute} onpin={pin} onanswer={answer} onaccept={accept} />
		{:else if showMembers && conversation}
			<MembersPanel {conversation} {ctx} {chatState} projectName={followsWords} onclose={() => (showMembers = false)} onmute={(id, mode) => muteIn(conversation.id, id, mode)} onrename={() => openChannelDialog('rename')} onaddpeople={() => openChannelDialog('members')} onremove={(id) => void act({ method: 'channel.remove_member', id: conversation.id, member_id: id })} onarchive={() => void act({ method: 'channel.archive', id: conversation.id }, `#${conversation.name} is archived.`)} />
		{/if}
	{/if}
</div>

<ChannelDialog bind:open={channelOpen} mode={channelMode} members={ctx.members} initialName={channelMode === 'rename' ? (conversation?.name ?? '') : ''} initialMembers={channelMode === 'members' ? (conversation?.members ?? []) : []} onsubmit={channelSubmit} />
<NewMessageDialog bind:open={dmOpen} members={ctx.members} onsubmit={dmSubmit} />
<CommandDialog bind:open={commandOpen} form={command} ondone={(message) => toast.success(message)} />
