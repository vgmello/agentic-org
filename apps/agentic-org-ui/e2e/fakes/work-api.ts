export { workSender } from '../../src/lib/modules/work/index';
export type { WorkDraft } from '../../src/lib/modules/work/index';
import { workSender } from '../../src/lib/modules/work/index';
import { createMockWork } from '../../src/lib/modules/work/mock';
import { orgApi } from '$lib/modules/organization/api';
import { isPocDataset } from './mock-mode';
import { pocWorkActivity, pocWorkItemById, pocWorkItems } from './poc-dataset';

const base = createMockWork(orgApi);

export const workApi: typeof base = {
	...base,
	async snapshot(organizationId, projectId, closedSince) {
		if (isPocDataset()) {
			const items = pocWorkItems().filter((i) => !projectId || i.project_id === projectId);
			return { position: { incarnation: 1, seq: items.length }, items };
		}
		return base.snapshot(organizationId, projectId, closedSince);
	},
	async get(organizationId, itemId) {
		if (isPocDataset()) {
			const item = pocWorkItemById(itemId);
			if (item) return item;
			throw { source: 'api', code: 'not_found', message: "That work item isn't here." };
		}
		return base.get(organizationId, itemId);
	},
	async activity(organizationId, itemId) {
		if (isPocDataset()) return pocWorkActivity(itemId);
		return base.activity(organizationId, itemId);
	}
};
export const sender = workSender(workApi);
if (typeof window !== 'undefined')
	(window as unknown as { __mockWorkAs: typeof workApi.commandAs }).__mockWorkAs =
		workApi.commandAs;
