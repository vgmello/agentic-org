// Intentional public surface; legacy API composition and mock internals stay private.
export { default as ChatSidebar } from './features/open-dm/ChatSidebar.svelte';
export { default as NewMessageDialog } from './features/open-dm/NewMessageDialog.svelte';
export { default as Composer } from './features/send-message/Composer.svelte';
export { default as MessageRow } from './components/MessageRow.svelte';
export { default as ThreadPanel } from './features/inspect-thread/ThreadPanel.svelte';
export { default as AskCard } from './features/answer-ask/AskCard.svelte';
export { default as ChannelDialog } from './features/manage-channel/ChannelDialog.svelte';
export { default as MembersPanel } from './features/manage-channel/MembersPanel.svelte';
export { default as CommandDialog } from './features/run-command/CommandDialog.svelte';
export { commandForm, turnForm, type CommandEnv, type CommandForm } from './commands';
export {
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
} from './model';
export type { ChatCall, ChatMessage, ChatSnapshot, Conversation, MuteMode, Reaction } from './types';
export { provideDraftStore, useDraftStore } from './drafts/context';
