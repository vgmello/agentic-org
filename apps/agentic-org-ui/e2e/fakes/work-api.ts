export { workSender } from '../../src/lib/work/index';
export type { WorkDraft } from '../../src/lib/work/index';
import { workSender } from '../../src/lib/work/index';
import { createMockWork } from '../../src/lib/work/api';
import { orgApi } from '$lib/org/api';
import { isPocDataset } from './mock-mode';
import { pocWorkItems } from './poc-dataset';

const base = createMockWork(orgApi);

export const workApi: typeof base = {
	...base,
	async snapshot(organizationId, projectId, closedSince) {
		if (isPocDataset()) {
			const items = pocWorkItems().filter((i) => !projectId || i.project_id === projectId);
			return { position: { incarnation: 1, seq: items.length }, items };
		}
		return base.snapshot(organizationId, projectId, closedSince);
	}
};

export const sender = workSender(workApi);
if (typeof window !== 'undefined')
	(window as unknown as { __mockWorkAs: typeof workApi.commandAs }).__mockWorkAs =
		workApi.commandAs;
