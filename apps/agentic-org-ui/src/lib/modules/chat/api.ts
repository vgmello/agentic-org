import {
	chatApi,
	chatSender,
	sender,
	type ChatApi,
	type ChatDraft
} from '$lib/modules/messaging/api';

/**
 * Production Chat API re-exported from `$lib/modules/messaging/api` so all callers share one
 * gateway singleton and dependency direction stays `chat -> messaging`.
 * Under `bun run dev:mock`, `e2e/fakes/modules/chat.ts` stands in for this file by name.
 */
export { chatApi, chatSender, sender, type ChatApi, type ChatDraft };
