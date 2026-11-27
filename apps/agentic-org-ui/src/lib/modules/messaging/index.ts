// Intentional public surface; legacy API composition and mock internals stay private.
export { default as ChatSidebar } from './features/open-dm/ChatSidebar.svelte';
export { default as NewMessageDialog } from './features/open-dm/NewMessageDialog.svelte';
export { default as Composer } from './features/send-message/Composer.svelte';
export { default as MessageRow } from './components/MessageRow.svelte';
export { default as ThreadPanel } from './features/inspect-thread/ThreadPanel.svelte';
export { default as AskCard } from './features/answer-ask/AskCard.svelte';
export { default as QuestionForm } from './features/answer-ask/QuestionForm.svelte';
export { default as HandoffCard } from './features/hand-off/HandoffCard.svelte';
export { default as ChannelDialog } from './features/manage-channel/ChannelDialog.svelte';
export { default as MembersPanel } from './features/manage-channel/MembersPanel.svelte';
export { default as CommandDialog } from './features/run-command/CommandDialog.svelte';
export { commandForm, turnForm, type CommandEnv, type CommandForm } from './commands';
export {
	OFFICE,
	SYSTEM_NO_REPLY,
	answerText,
	optionsOf,
	questionsOf,
	type ChatContext
} from './engine';
export {
	SYSTEM_NAME,
	browseChannels,
	cannotRunFor,
	cannotRunWords,
	chatContext,
	composerState,
	conversationView,
	dayLabel,
	handoffOutcome,
	locate,
	nameOf,
	outcomeWords,
	parseChatLocation,
	parseSearch,
	parseSlash,
	pinsIn,
	presenceOf,
	repliesTo,
	requiredWords,
	searchMessages,
	sidebar,
	titleOf,
	typingWords,
	unreadIn,
	whoOf,
	type BrowseRow,
	type ChatView,
	type ComposerState,
	type ConversationView,
	type Sidebar
} from './model';
export type {
	AskOption,
	AskQuestion,
	ChatCall,
	ChatCallStatus,
	ChatMessage,
	ChatSnapshot,
	ChatState,
	Conversation,
	MuteMode,
	QuestionAnswer,
	Reaction
} from './types';
export { provideDraftStore, useDraftStore } from './drafts/context';

export { messageRows, type MessageRows } from './row-data';

export { RowActivity } from './row-activity.svelte';
