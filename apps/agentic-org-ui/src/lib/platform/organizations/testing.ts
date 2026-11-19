import { vi } from 'vitest';
import { createOrganizationsApi } from './api';

/** Test fixtures shared by this module's tests; never imported by production code. */
export const INSTANCE = '0193a5c0-0000-7000-8000-000000000009';
export const OP = '01M72N8MG00000000000000006';
export const OP2 = '01M72N8MG00000000000000007';
export const office = {
	id: '0193a5c0-0000-7000-8000-000000000002',
	organization_id: '0193a5c0-0000-7000-8000-000000000001',
	name: 'HQ',
	role: 'hq',
	host: 'This Mac',
	status: 'starting'
};
export const organization = {
	id: '0193a5c0-0000-7000-8000-000000000001',
	slug: 'side-project',
	name: 'Side Project',
	created_at: '2026-11-10T12:00:00.000Z',
	employee_id: '0193a5c0-0000-7000-8000-000000000006',
	recovery: 'pending',
	offices: [office]
};
export const params = {
	name: 'Side Project',
	slug: 'side-project',
	repository: 'https://example.test/org/state.git',
	stateBranch: 'main'
};
export const fields = { ...params };
type Reply = { status: number; body: unknown };
export const accepted = (id = OP): Reply => ({
	status: 202,
	body: { outcome: 'accepted', operation_id: id }
});
export const completed = (id = OP): Reply => ({
	status: 200,
	body: { outcome: 'completed', operation_id: id, result: organization }
});
export const rejected = (status: number, error: Record<string, unknown>, id = OP): Reply => ({
	status,
	body: { outcome: 'rejected', operation_id: id, error: { message: 'm', ...error } }
});
export const unknown = (id = OP): Reply => ({
	status: 504,
	body: { outcome: 'unknown', operation_id: id }
});
export const looked = (inner: Record<string, unknown>, id = OP): Reply => ({
	status: 200,
	body: { result: { operation_id: id, kind: 'host', ...inner } }
});
export const lookedAccepted = (id = OP) => looked({ outcome: 'accepted', state: 'reserved' }, id);
export const lookedCompleted = (id = OP) =>
	looked({ outcome: 'completed', state: 'linked', result: organization }, id);
export const notFound: Reply = {
	status: 404,
	body: { error: { code: 'not_found', message: 'Not found.' } }
};

/** An API over scripted replies. Each call takes the next reply of its queue. */
export function scripted(ids: string[] = [OP, OP2]) {
	const read = vi.fn<(path: string, signal?: AbortSignal) => Promise<Reply>>();
	const submit = vi.fn<(path: string, body: unknown, signal?: AbortSignal) => Promise<Reply>>();
	const minted = [...ids];
	const newId = vi.fn(() => minted.shift() ?? 'none left');
	const api = createOrganizationsApi({ http: { read, submit }, instanceId: () => INSTANCE, newId });
	return { api, read, submit, newId };
}
