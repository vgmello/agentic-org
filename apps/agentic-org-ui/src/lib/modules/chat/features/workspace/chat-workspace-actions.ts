import { toast } from 'svelte-sonner';
import {
	commandForm,
	nameOf,
	parseSlash,
	turnForm,
	type ChatContext,
	type ChatMessage,
	type ChatState,
	type CommandEnv,
	type CommandForm,
	type Conversation,
	type MuteMode,
	type QuestionAnswer,
	type Reaction
} from '$lib/modules/messaging';
import { newId } from '$lib/modules/organization/commands';
import { toAppError } from '$lib/proto';
import type { ChatApi, sender } from '../../api';
import type { PendingMessage } from '../../types';
import {
	checkPendingMessageStatus,
	deliverPendingMessage,
	restoreTemporaryFocus
} from './chat-workspace-helpers';

export interface ChatActionDeps {
	chatApi: ChatApi;
	getOrganizationId: () => string;
	getSend: () => ReturnType<typeof sender>;
	getCtx: () => ChatContext;
	getChatState?: () => ChatState | undefined;
	getEnv: () => CommandEnv;
	getOfficeRunning: () => boolean;
	getListEl: () => HTMLElement | undefined;
	focusPlace: () => string;
	load: () => Promise<void>;
	pushPending: (item: PendingMessage) => PendingMessage;
	removePending: (key: number) => void;
	openCommandForm: (form: CommandForm | { error: string }) => boolean;
	onTurnHeld: (messageId: string, opener: HTMLElement) => void;
	onSelectConversation?: (id: string) => void;
}

export function createChatMessageActions(deps: ChatActionDeps) {
	let keys = 0;

	async function act(
		draft: Parameters<ReturnType<typeof sender>>[0],
		success?: string,
		quiet = false
	) {
		try {
			const accepted = await deps.getSend()(draft);
			await deps.load();
			if (success) toast.success(success);
			return accepted;
		} catch (e) {
			if (!quiet) toast.error(toAppError(e).message);
			return undefined;
		}
	}

	async function cardCall(draft: Parameters<ReturnType<typeof sender>>[0]) {
		if (!deps.getOfficeRunning()) {
			throw new Error("The office isn't running. Restart it from Offices.");
		}
		try {
			await deps.getSend()(draft);
		} catch (e) {
			await deps.load();
			throw toAppError(e);
		}
		await deps.load();
	}

	const deliver = (p: PendingMessage, retry = false, retryButton?: HTMLElement) =>
		deliverPendingMessage({
			chatApi: deps.chatApi,
			organizationId: deps.getOrganizationId(),
			p,
			retry,
			retryButton,
			focusPlace: deps.focusPlace,
			removePending: deps.removePending,
			reload: deps.load,
			listEl: deps.getListEl
		});

	const checkStatus = (p: PendingMessage) =>
		checkPendingMessageStatus({
			chatApi: deps.chatApi,
			organizationId: deps.getOrganizationId(),
			p,
			removePending: deps.removePending,
			reload: deps.load
		});

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
			return deps.openCommandForm(commandForm(slash, deps.getEnv()));
		}
		const pushed = deps.pushPending({
			key: ++keys,
			change_id: newId(),
			conversation_id: c.id,
			thread_root_id: thread,
			also_in_channel: thread && options.alsoInChannel ? true : undefined,
			body: text.trim(),
			attachments: options.attachments,
			status: 'sending'
		});
		void deliver(pushed);
		return true;
	}

	const react = (m: ChatMessage, reaction: Reaction) => {
		if (deps.getOfficeRunning()) void act({ method: 'chat.react', message_id: m.id, reaction });
	};

	const pin = (m: ChatMessage) => {
		if (deps.getOfficeRunning()) void act({ method: 'chat.pin', message_id: m.id });
	};

	const muteIn = (conversation_id: string, member_id: string, mode: MuteMode) => {
		if (!deps.getOfficeRunning()) return;
		const who = nameOf(deps.getCtx(), member_id);
		const notice =
			mode === 'none'
				? `${who} is back.`
				: mode === 'muted'
					? `${who} is muted here. Their messages still reach Activity.`
					: `${who} is on a daily digest here.`;
		void act({ method: 'chat.mute', conversation_id, member_id, mode }, notice);
	};

	const mute = (m: ChatMessage, mode: MuteMode) => muteIn(m.conversation_id, m.from, mode);

	const turn = (m: ChatMessage, into: 'bug' | 'request' | 'task', opener: HTMLElement) => {
		if (deps.openCommandForm(turnForm(into, m, deps.getEnv()))) {
			deps.onTurnHeld(m.id, opener);
		}
	};

	async function answer(
		m: ChatMessage,
		a: { option_id?: string; text?: string; answers?: QuestionAnswer[] }
	) {
		const origin = document.activeElement;
		const area = origin?.closest('[data-focus-scope]');
		const place = deps.focusPlace();
		await cardCall({ method: 'chat.answer', ask_id: m.id, ...a });
		toast.success(`Answer sent to ${nameOf(deps.getCtx(), m.from)}.`);
		await restoreTemporaryFocus(
			origin,
			place,
			deps.focusPlace,
			() => area?.querySelector<HTMLElement>(`[data-ask-id="${CSS.escape(m.id)}"]`) ?? null,
			deps.getListEl
		);
	}

	const accept = (m: ChatMessage) => cardCall({ method: 'chat.accept_handoff', message_id: m.id });

	const handBack = async (m: ChatMessage, reason: string) => {
		await cardCall({ method: 'chat.hand_back', message_id: m.id, reason });
		toast.success(`Handed back to ${nameOf(deps.getCtx(), m.from)}.`);
	};

	const withdraw = async (m: ChatMessage, reason: string) => {
		await cardCall({ method: 'chat.withdraw', ask_id: m.id, reason });
		toast.success('Ask withdrawn.');
	};

	const removeMember = (id: string, member_id: string) => {
		if (deps.getOfficeRunning()) void act({ method: 'channel.remove_member', id, member_id });
	};

	const archiveChannel = (c: Conversation) => {
		if (deps.getOfficeRunning()) {
			void act({ method: 'channel.archive', id: c.id }, `#${c.name} is archived.`);
		}
	};

	const openDm = async (memberId: string) => {
		const owner = deps.getCtx().owner;
		const existing = deps
			.getChatState?.()
			?.conversations.find(
				(c) =>
					c.kind === 'dm' &&
					c.members.length === 2 &&
					c.members.includes(owner) &&
					c.members.includes(memberId)
			);
		if (existing) {
			deps.onSelectConversation?.(existing.id);
			return;
		}
		if (!deps.getOfficeRunning()) return;
		const accepted = await act({ method: 'dm.open', members: [memberId] });
		if (accepted?.conversation_id) deps.onSelectConversation?.(accepted.conversation_id);
	};

	return {
		act,
		deliver,
		checkStatus,
		submit,
		react,
		pin,
		muteIn,
		mute,
		turn,
		answer,
		accept,
		handBack,
		withdraw,
		removeMember,
		archiveChannel,
		openDm
	};
}

export type ChatMessageActions = ReturnType<typeof createChatMessageActions>;
