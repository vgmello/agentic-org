// Intentional public surface; legacy API composition and mock internals stay private.
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
export { provideDraftStore, useDraftStore, useOptionalDraftStore } from './drafts/context';

export { messageRows, type MessageRows } from './row-data';

export { RowActivity } from './row-activity.svelte';
