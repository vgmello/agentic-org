import { chatSender, type ChatApi, type ChatDraft } from '$lib/modules/messaging/api';
import { unavailableCall } from '$lib/platform/unavailable-api';

export { chatSender, type ChatApi, type ChatDraft };

/**
 * Production fail-closed Chat API until B6/B7 messaging/asks backend routes land.
 * Under `bun run dev:mock`, `e2e/fakes/modules/chat.ts` stands in for this file by name.
 */
export const chatApi: ChatApi = Object.freeze({
	snapshot: unavailableCall,
	command: unavailableCall,
	status: unavailableCall,
	subscribe: () => () => {}
});

export const sender = chatSender(chatApi);
