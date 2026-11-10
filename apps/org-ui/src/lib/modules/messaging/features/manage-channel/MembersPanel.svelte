<script lang="ts">
	import X from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import type { ChatContext } from '$lib/modules/messaging/engine';
	import { muteOf, PRESENCE_WORD, titleOf } from '$lib/modules/messaging/model';
	import type { ChatState, Conversation, MuteMode, Presence } from '$lib/modules/messaging/types';
	import Avatar from '$lib/modules/messaging/components/Avatar.svelte';

	/** Who is in a conversation and how the owner hears from them (CHAT-11); channels people made can be edited here (CHAT-2). */
	let {
		conversation,
		ctx,
		chatState,
		sidebarInline = true,
		officeRunning = true,
		projectName,
		onclose,
		onmute,
		onrename,
		onaddpeople,
		onremove,
		onarchive
	}: {
		conversation: Conversation;
		ctx: ChatContext;
		chatState: ChatState;
		/** Whether the conversation list occupies its inline 256px column. */
		sidebarInline?: boolean;
		officeRunning?: boolean;
		/** What an automatic channel follows, in words: "the Atlas project". */
		projectName: (c: Conversation) => string | undefined;
		onclose: () => void;
		onmute: (memberId: string, mode: MuteMode) => void;
		onrename: () => void;
		onaddpeople: () => void;
		onremove: (memberId: string) => void;
		onarchive: () => void;
	} = $props();

	const members = $derived(conversation.members.map((id) => ctx.members.find((m) => m.id === id)).filter((m) => !!m));
	const follows = $derived(projectName(conversation));
	const editable = $derived(conversation.kind === 'channel' && !conversation.auto && !conversation.archived);
	const MODES: { id: MuteMode; label: string }[] = [
		{ id: 'none', label: 'All messages' },
		{ id: 'digest', label: 'Daily digest' },
		{ id: 'muted', label: 'Muted' }
	];
	const presenceOf = (id: string, kind: 'agent' | 'person'): Presence => chatState.presence[id] ?? (kind === 'agent' ? 'offline' : 'online');
	let closeButton = $state<HTMLButtonElement | HTMLAnchorElement | null>(null);
	export function focusClose() {
		closeButton?.focus({ preventScroll: true });
		closeButton?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
	}
</script>

<aside class="absolute inset-y-0 right-0 {sidebarInline ? 'left-64' : 'left-0'} z-30 flex h-full min-w-0 shrink-0 flex-col shadow-xl xl:static xl:w-96 xl:shadow-none border-l border-white/10 bg-gray-800" aria-label="Members" data-testid="members-panel">
	<div class="flex items-center justify-between gap-3 border-b border-white/10 px-4 py-3">
		<h2 class="min-w-0 text-sm/6 font-semibold break-words text-white">{titleOf(conversation, ctx, ctx.owner)} <span class="font-normal text-gray-400">· {members.length} {members.length === 1 ? 'member' : 'members'}</span></h2>
		<Button type="button" variant="ghost" size="icon-sm" class="scroll-mt-16" aria-label="Close members" bind:ref={closeButton} onclick={onclose}><X aria-hidden="true" /></Button>
	</div>
	<div class="min-h-0 flex-1 space-y-4 overflow-y-auto px-4 py-3">
		{#if follows}<p class="rounded-md bg-white/5 px-3 py-2 text-sm/6 text-gray-300">This channel follows {follows}: its members and name change with it.</p>{/if}
		<ul class="space-y-3">
			{#each members as m (m.id)}
				<li class="flex items-center gap-x-3 max-[640px]:flex-wrap max-[640px]:gap-y-2" data-testid="member-row">
					<Avatar name={m.name} kind={m.kind} presence={presenceOf(m.id, m.kind)} />
					<div class="min-w-0 flex-1 max-[640px]:basis-[200px]">
						<p class="truncate text-sm/6 font-semibold text-white">{m.name}{#if m.owner}{' '}<span class="font-normal text-gray-400">(you)</span>{/if}</p>
						<p class="text-xs/5 break-words text-gray-400">{m.kind === 'agent' ? 'Agent' : m.owner ? 'Owner' : 'Person'} · {PRESENCE_WORD[presenceOf(m.id, m.kind)]} · @{m.handle}</p>
					</div>
					{#if m.kind === 'agent' && conversation.kind !== 'dm'}
						<label class="sr-only" for="mute-{m.id}">How you hear from {m.name} here</label>
						<select disabled={!officeRunning} id="mute-{m.id}" class="rounded-md bg-gray-900 disabled:opacity-50 px-2 py-1 text-xs/5 text-white outline-1 -outline-offset-1 outline-white/10" value={muteOf(chatState, conversation.id, m.id)} onchange={(e) => onmute(m.id, e.currentTarget.value as MuteMode)} data-testid="mute-select">
							{#each MODES as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
						</select>
					{/if}
					{#if editable && !m.owner}
						<Button type="button" variant="ghost" size="xs" aria-label="Remove {m.name} from the channel" onclick={() => onremove(m.id)}>Remove</Button>
					{/if}
				</li>
			{/each}
		</ul>
		{#if editable}
			<div class="flex flex-wrap gap-2">
				<Button type="button" size="sm" variant="outline" onclick={onaddpeople}>Add people…</Button>
				<Button type="button" size="sm" variant="outline" onclick={onrename}>Rename…</Button>
				<Button type="button" size="sm" variant="outline" onclick={onarchive}>Archive</Button>
			</div>
		{/if}
		<p class="text-xs/5 text-gray-400">Muting keeps an agent's messages out of this conversation. They still reach Activity.</p>
	</div>
</aside>
