import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { createHttpProcessesApi } from './http-api';

const snapshot: OrgSnapshot = {
	position: { epoch: 1, seq: 3 },
	teams: [{ id: 't-eng', name: 'Engineering', version: 1 }],
	members: [{ id: 'm-owner', kind: 'person', name: 'Vinicius', owner: true, version: 1 }],
	projects: [{ id: 'p-core', key: 'CORE', name: 'Core', version: 1 }],
	project_teams: []
};

describe('createHttpProcessesApi', () => {
	it('loads organization processes, standards, and creates a document', async () => {
		const read = vi.fn(async (path: string) => {
			if (path.includes('/processes?level=organization')) {
				return {
					status: 200,
					body: {
						result: {
							scope: { level: 'organization' },
							scope_name: 'Organization',
							computed_at: '2026-11-26T04:00:00Z',
							inactive: [],
							problems: [],
							rows: [
								{
									id: 'doc-1',
									key: 'code-review',
									kind: 'process',
									applies: true,
									mandatory: true,
									document: {
										id: 'doc-1',
										key: 'code-review',
										revision: 1,
										kind: 'process',
										title: 'Code Review',
										status: 'current',
										owner_employee_id: 'm-owner',
										level: 'organization',
										overridable: true,
										summary: 'Review every PR',
										body_markdown: '# Code Review',
										path: 'processes/code-review.md',
										created_at: '2026-11-25T10:00:00Z',
										created_by_kind: 'employee',
										created_by_id: 'm-owner',
										updated_at: '2026-11-25T10:00:00Z',
										updated_by_kind: 'employee',
										updated_by_id: 'm-owner'
									},
									chain: []
								}
							]
						}
					}
				};
			}
			return { status: 404, body: {} };
		});

		const submit = vi.fn(async () => ({
			status: 200,
			body: {
				result: {
					document_id: 'doc-2',
					revision: 1,
					document: {
						id: 'doc-2',
						key: 'release-check',
						revision: 1,
						kind: 'standard',
						title: 'Release Check',
						status: 'draft',
						level: 'organization',
						overridable: true,
						summary: '',
						body_markdown: 'Check release',
						path: 'standards/release-check.md',
						created_at: '2026-11-26T04:00:00Z',
						created_by_kind: 'employee',
						updated_at: '2026-11-26T04:00:00Z',
						updated_by_kind: 'employee'
					}
				}
			}
		}));

		const api = createHttpProcessesApi({
			http: { read, submit },
			orgApi: { view: async () => ({ snapshot, position: snapshot.position, sample: false }) },
			session: () => ({ accountAdmin: true })
		});

		const view = await api.view('org-1', { kind: 'organization' });
		expect(view.processes).toHaveLength(1);
		expect(view.processes[0]?.document.title).toBe('Code Review');
		expect(view.viewer.canEdit).toBe(true);
		expect(view.options.map((o) => o.name)).toContain('Engineering (team)');

		const created = await api.create('org-1', {
			kind: 'standard',
			title: 'Release Check',
			key: 'release-check',
			status: 'draft',
			overridable: true,
			bodyMarkdown: 'Check release'
		});
		expect(created.id).toBe('doc-2');
		expect(submit).toHaveBeenCalledWith(
			'/api/v1/organizations/org-1/processes',
			expect.objectContaining({
				kind: 'standard',
				title: 'Release Check',
				scope: { level: 'organization' }
			})
		);
	});
});
