<!--
  @component
  Modal dialogs for ChatWorkspace (ChannelDialog, NewMessageDialog, BrowseChannelsDialog, CommandDialog).
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import {
		type BrowseRow,
		type ChatContext,
		type CommandForm,
		type Conversation
	} from '$lib/modules/messaging';
	import { ChannelDialog, CommandDialog, NewMessageDialog } from '$lib/modules/messaging/ui';
	import type { sender } from '../../api';
	import BrowseChannelsDialog from '../browse-channels/BrowseChannelsDialog.svelte';

	let {
		ctx,
		conversation,
		officeRunning,
		browseRows,
		send,
		load,
		channelMode,
		channelOpen = $bindable(false),
		dmOpen = $bindable(false),
		browseOpen = $bindable(false),
		commandOpen = $bindable(false),
		command,
		sidebarDialogMode,
		hasCommandOpener,
		onFinishSidebarDialog,
		onBrowseReturn,
		onBrowseShow,
		onReturnCommandFocus,
		onSelectConversation
	}: {
		ctx: ChatContext;
		conversation: Conversation | undefined;
		officeRunning: boolean;
		browseRows: BrowseRow[];
		send: ReturnType<typeof sender>;
		load: () => Promise<void>;
		channelMode: 'create' | 'rename' | 'members';
		channelOpen: boolean;
		dmOpen: boolean;
		browseOpen: boolean;
		commandOpen: boolean;
		command: CommandForm | undefined;
		sidebarDialogMode: boolean;
		hasCommandOpener: boolean;
		onFinishSidebarDialog: () => void;
		onBrowseReturn: () => void;
		onBrowseShow: (id: string) => void;
		onReturnCommandFocus: () => void;
		onSelectConversation: (id: string) => void;
	} = $props();

	let channelDialog = $state<{ dismiss: () => void } | undefined>();
	let dmDialog = $state<{ dismiss: () => void } | undefined>();
	let browseDialog = $state<{ dismiss: () => void } | undefined>();

	export function dismissOpenDialogs() {
		if (browseOpen) browseDialog?.dismiss();
		if (channelOpen) channelDialog?.dismiss();
		if (dmOpen) dmDialog?.dismiss();
		commandOpen = false;
	}

	function requireRunningOffice() {
		if (!officeRunning) throw new Error("The office isn't running. Restart it from Offices.");
	}

	async function channelSubmit(r: { name: string; members: string[] }) {
		requireRunningOffice();
		if (channelMode === 'create') {
			const accepted = await send({ method: 'channel.create', name: r.name, members: r.members });
			await load();
			if (accepted.conversation_id) onSelectConversation(accepted.conversation_id);
			return;
		}
		if (!conversation) return;
		if (channelMode === 'rename') {
			await send({ method: 'channel.rename', id: conversation.id, name: r.name });
			await load();
			return;
		}
		for (const id of r.members.filter((x) => !conversation.members.includes(x))) {
			await send({ method: 'channel.add_member', id: conversation.id, member_id: id });
		}
		for (const id of conversation.members.filter(
			(x) => !r.members.includes(x) && x !== ctx.owner
		)) {
			await send({ method: 'channel.remove_member', id: conversation.id, member_id: id });
		}
		await load();
	}

	async function browseJoin(id: string) {
		requireRunningOffice();
		await send({ method: 'channel.join', id });
		await load();
		const row = browseRows.find((x) => x.conversation.id === id);
		toast.success(`You joined #${row?.conversation.name ?? 'the channel'}.`);
		onBrowseShow(id);
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
		if (accepted.conversation_id) onSelectConversation(accepted.conversation_id);
	}
</script>

<ChannelDialog
	bind:this={channelDialog}
	returnFocus={sidebarDialogMode ? onFinishSidebarDialog : undefined}
	bind:open={channelOpen}
	mode={channelMode}
	members={ctx.members}
	initialName={channelMode === 'rename' ? (conversation?.name ?? '') : ''}
	initialMembers={channelMode === 'members' ? (conversation?.members ?? []) : []}
	onsubmit={channelSubmit}
/>
<NewMessageDialog
	bind:this={dmDialog}
	returnFocus={sidebarDialogMode ? onFinishSidebarDialog : undefined}
	bind:open={dmOpen}
	members={ctx.members}
	onsubmit={dmSubmit}
/>
<BrowseChannelsDialog
	bind:this={browseDialog}
	bind:open={browseOpen}
	rows={browseRows}
	{officeRunning}
	returnFocus={onBrowseReturn}
	onopen={onBrowseShow}
	onjoin={browseJoin}
	onrequest={browseRequest}
/>
<CommandDialog
	returnFocus={hasCommandOpener ? onReturnCommandFocus : undefined}
	bind:open={commandOpen}
	form={command}
	ondone={(message) => toast.success(message)}
/>
