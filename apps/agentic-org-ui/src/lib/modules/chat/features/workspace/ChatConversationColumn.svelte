<!--
  @component
  Main conversation column (header, status callouts, MessageFeed, typing indicator, Composer) for ChatWorkspace.
-->
<script lang="ts">
	import Info from '@lucide/svelte/icons/info';
	import Unplug from '@lucide/svelte/icons/unplug';
	import Callout from '$lib/components/Callout.svelte';
	import {
		Composer,
		MessageRow,
		titleOf,
		type ChatContext,
		type ChatMessage,
		type ChatState,
		type ChatView,
		type ComposerState,
		type Conversation,
		type ConversationView,
		type MessageRows,
		type Sidebar
	} from '$lib/modules/messaging';
	import { pendingToMessage } from '../../helpers';
	import type { PendingMessage } from '../../types';
	import type { ChatMessageActions } from './chat-workspace-actions';
	import ConversationHeader from './ConversationHeader.svelte';
	import MessageFeed from './MessageFeed.svelte';

	let {
		conversationEl = $bindable<HTMLElement | undefined>(),
		listEl = $bindable<HTMLElement | undefined>(),
		heading = $bindable<HTMLHeadingElement | undefined>(),
		sidebarToggle = $bindable<HTMLButtonElement | HTMLAnchorElement | null>(null),
		showMuted = $bindable(false),
		openDigest = $bindable<string | undefined>(),
		inert,
		viewTitle,
		subtitle,
		conversation,
		searching,
		sample,
		sidebarInline,
		sidebarOpen,
		conversationsId,
		inboxCount,
		pinCount,
		showPins,
		showMembers,
		query,
		bridgePreview,
		officeRunning,
		copyAge,
		results,
		view,
		bar,
		convView,
		chatState,
		ctx,
		rows,
		now,
		newAt,
		highlight,
		typing,
		compose,
		cannotRun,
		pendingHere,
		actions,
		rowIsActive,
		onmenu,
		channelRowLive,
		onOpenSidebar,
		onTogglePins,
		onToggleMembers,
		onQueryChange,
		onOpenThread,
		onOpenMessage,
		onRemovePending
	}: {
		conversationEl: HTMLElement | undefined;
		listEl: HTMLElement | undefined;
		heading: HTMLHeadingElement | undefined;
		sidebarToggle: HTMLButtonElement | HTMLAnchorElement | null;
		showMuted: boolean;
		openDigest: string | undefined;
		inert: boolean;
		viewTitle: string;
		subtitle: string;
		conversation: Conversation | undefined;
		searching: boolean;
		sample: boolean;
		sidebarInline: boolean;
		sidebarOpen: boolean;
		conversationsId: string;
		inboxCount: number | undefined;
		pinCount: number;
		showPins: boolean;
		showMembers: boolean;
		query: string;
		bridgePreview: boolean;
		officeRunning: boolean;
		copyAge: string | undefined;
		results: ChatMessage[];
		view: ChatView;
		bar: Sidebar;
		convView: ConversationView | undefined;
		chatState: ChatState;
		ctx: ChatContext;
		rows: MessageRows;
		now: string;
		newAt: string | undefined;
		highlight: string | undefined;
		typing: string | undefined;
		compose: ComposerState | undefined;
		cannotRun: readonly string[];
		pendingHere: PendingMessage[];
		actions: ChatMessageActions;
		rowIsActive: (m: ChatMessage) => boolean;
		onmenu: (message: ChatMessage, open: boolean, opener: HTMLElement) => void;
		channelRowLive: (m: ChatMessage) => 'off' | undefined;
		onOpenSidebar: () => void;
		onTogglePins: (opener: HTMLElement) => void;
		onToggleMembers: (opener: HTMLElement) => void;
		onQueryChange: (value: string) => void;
		onOpenThread: (m: ChatMessage, opener: HTMLElement) => void;
		onOpenMessage: (m: ChatMessage) => void;
		onRemovePending: (key: number) => void;
	} = $props();
</script>

{#snippet rowFor(m: ChatMessage, extra: { where?: string; open?: boolean } = {})}
	<MessageRow
		message={m}
		{ctx}
		rowData={rows.forMessage(m)}
		{now}
		readOnly={bridgePreview}
		{officeRunning}
		active={rowIsActive(m)}
		{onmenu}
		where={extra.where}
		onreact={actions.react}
		onthread={onOpenThread}
		onturn={actions.turn}
		onmute={actions.mute}
		onpin={actions.pin}
		onanswer={actions.answer}
		onaccept={actions.accept}
		onhandback={actions.handBack}
		onwithdraw={actions.withdraw}
		onopen={extra.open ? onOpenMessage : undefined}
	/>
{/snippet}

<section
	bind:this={conversationEl}
	{inert}
	class="chat-canvas flex min-w-0 flex-1 flex-col overflow-y-auto"
	aria-label="Conversation"
>
	<ConversationHeader
		{viewTitle}
		{subtitle}
		{conversation}
		{searching}
		{sample}
		{sidebarInline}
		{sidebarOpen}
		{conversationsId}
		{inboxCount}
		{pinCount}
		{showPins}
		{showMembers}
		{query}
		bind:heading
		bind:sidebarToggle
		onopensidebar={onOpenSidebar}
		ontogglepins={onTogglePins}
		ontogglemembers={onToggleMembers}
		onquerychange={onQueryChange}
	/>

	{#if bridgePreview}
		<Callout data-testid="bridge-preview">
			{#snippet icon()}<Info class="size-4" aria-hidden="true" />{/snippet}
			<p>
				Previewing how a bridge (Slack, Teams or Discord) shows this: read-only here, and asks to
				the owner and approvals open in Agentic Org.
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
		onopenthread={onOpenThread}
	>
		{#snippet undelivered()}
			{#each pendingHere as p (p.key)}
				{@const message = pendingToMessage(p, ctx.owner, now)}
				<MessageRow
					{message}
					{ctx}
					rowData={rows.forMessage(message)}
					{now}
					{officeRunning}
					delivery={{ status: p.status, error: p.error }}
					oncheck={() => void actions.checkStatus(p)}
					onretry={(button) => void actions.deliver(p, true, button)}
					ondismiss={() => onRemovePending(p.key)}
				/>
			{/each}
		{/snippet}
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
			onsend={(t, o) => actions.submit(t, o, conversation, undefined)}
		/>
	{/if}
</section>
