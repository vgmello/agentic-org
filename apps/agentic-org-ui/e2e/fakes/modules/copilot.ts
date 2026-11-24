export {
	copilotFeatures,
	describeCopilotFailure,
	forgetCopilotFeatures,
	offers,
	type CopilotApi
} from '../../../src/lib/modules/copilot/api';
import { createMockCopilot } from '../../../src/lib/modules/copilot/mock';

export const copilotApi = createMockCopilot();
