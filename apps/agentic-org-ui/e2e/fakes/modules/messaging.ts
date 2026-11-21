export { chatSender, type ChatApi, type ChatDraft } from '../../../src/lib/modules/messaging/api';
export {
	createMockChat,
	type ChatScenario,
	type MockChatOptions
} from '../../../src/lib/modules/messaging/mock';
import { orgApi } from '$lib/org/api';
import { chatSender } from '../../../src/lib/modules/messaging/api';
import { createMockChat } from '../../../src/lib/modules/messaging/mock';

export const chatApi = createMockChat(orgApi);
export const sender = chatSender(chatApi);
