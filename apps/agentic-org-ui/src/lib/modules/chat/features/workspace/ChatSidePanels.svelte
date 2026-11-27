<!--
  @component
  Right-hand side panel switcher (ThreadPanel, MembersPanel, PinsPanel) for ChatWorkspace.
-->
<script lang="ts">
	import type { OrgSnapshot } from '$lib/proto';
	import {
		MembersPanel,
		MessageRow,
		ThreadPanel,
		type ChatContext,
		type ChatMessage,
		type ChatState,
		type Conversation,
		type MessageRows
	} from '$lib/modules/messaging';
	import { followsWords, pendingToMessage } from '../../helpers';
	import type { PendingMessage } from '../../types';
	import PinsPanel from '../pin-message/PinsPanel.svelte';
	import type { ChatMessageActions } from './chat-workspace-actions';

	let {
		root,
		threadConversation,
		showMembers,
		showPins,
		conversation,
		rows,
		sidebarInline,
		ctx,
		chatState,
		snapshot,
		pins,
		now,
		officeRunning,
		cannotRun,
		bridgePreview,
		pending,
		rowIsActive,
		actions,
		onmenu,
		onCloseThread,
		onCloseMembers,
		onClosePins,
		onRenameChannel,
		onAddPeople,
		onShowPinned,
		onRemovePending
	}: {
		root: ChatMessage | undefined;
		threadConversation: Conversation | undefined;
		showMembers: boolean;
		showPins: boolean;
		conversation: Conversation | undefined;
		rows: MessageRows;
		sidebarInline: boolean;
		ctx: ChatContext;
		chatState: ChatState;
		snapshot: OrgSnapshot;
		pins: ChatMessage[];
		now: string;
		officeRunning: boolean;
		cannotRun: readonly string[];
		bridgePreview: boolean;
		pending: PendingMessage[];
		rowIsActive: (m: ChatMessage) => boolean;
		actions: ChatMessageActions;
		onmenu: (message: ChatMessage, open: boolean, opener: HTMLElement) => void;
		onCloseThread: () => void;
		onCloseMembers: () => void;
		onClosePins: () => void;
		onRenameChannel: () => void;
		onAddPeople: () => void;
		onShowPinned: (m: ChatMessage) => void;
		onRemovePending: (key: number) => void;
	} = $props();

	let activePanel = $state<{ focusClose: () => void } | undefined>();

	export function focusClose() {
		activePanel?.focusClose();
	}
</script>

{#if root && threadConversation}
	<ThreadPanel
		active={rowIsActive}
		{onmenu}
		{rows}
		{sidebarInline}
		bind:this={activePanel}
		{root}
		conversation={threadConversation}
		{ctx}
		{chatState}
		{now}
		{officeRunning}
		{cannotRun}
		readOnly={bridgePreview}
		onclose={onCloseThread}
		onsend={(t, o) => actions.submit(t, o, threadConversation, root.id)}
		onreact={actions.react}
		onturn={actions.turn}
		onmute={actions.mute}
		onpin={actions.pin}
		onanswer={actions.answer}
		onaccept={actions.accept}
		onhandback={actions.handBack}
		onwithdraw={actions.withdraw}
	>
		{#snippet undelivered()}
			{#each pending.filter((p) => p.thread_root_id === root.id) as p (p.key)}
				{@const message = pendingToMessage(p, ctx.owner, now)}
				<MessageRow
					{message}
					{ctx}
					rowData={rows.forMessage(message)}
					{now}
					inThread={true}
					{officeRunning}
					delivery={{ status: p.status, error: p.error }}
					oncheck={() => void actions.checkStatus(p)}
					onretry={(button) => void actions.deliver(p, true, button)}
					ondismiss={() => onRemovePending(p.key)}
				/>
			{/each}
		{/snippet}
	</ThreadPanel>
{:else if showMembers && conversation}
	<MembersPanel
		{sidebarInline}
		bind:this={activePanel}
		{conversation}
		{ctx}
		{chatState}
		{officeRunning}
		projectName={(c) => followsWords(c, snapshot)}
		onclose={onCloseMembers}
		onmute={(id, mode) => actions.muteIn(conversation.id, id, mode)}
		onrename={onRenameChannel}
		onaddpeople={onAddPeople}
		onremove={(id) => actions.removeMember(conversation.id, id)}
		onarchive={() => actions.archiveChannel(conversation)}
	/>
{:else if showPins && conversation}
	<PinsPanel
		{sidebarInline}
		bind:this={activePanel}
		{conversation}
		{ctx}
		{pins}
		{now}
		{officeRunning}
		onclose={onClosePins}
		onshow={onShowPinned}
		onunpin={actions.pin}
	/>
{/if}
