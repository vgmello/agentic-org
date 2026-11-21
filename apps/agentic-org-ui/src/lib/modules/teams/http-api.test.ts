import { describe, expect, it, vi } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { createHttpTeamsApi } from './http-api';

const ORG = '0193a5c0-0000-7000-8000-000000000001';
const ROOT = '0193a5c0-0000-7000-8000-000000000002';
const ENG = '0193a5c0-0000-7000-8000-000000000003';
const OWNER = '0193a5c0-0000-7000-8000-000000000004';
const ADA = '0193a5c0-0000-7000-8000-000000000005';
const PROJ = '0193a5c0-0000-7000-8000-000000000006';

const SNAPSHOT: OrgSnapshot = {
	position: { epoch: 1, seq: 3 },
	teams: [
		{ id: ROOT, name: 'Acme', version: 1 },
		{
			id: ENG,
			name: 'Engineering',
			owner_team_id: ROOT,
			lead_member_id: OWNER,
			version: 2
		}
	],
	members: [
		{
			id: OWNER,
			kind: 'person',
			name: 'Alex Owner',
			handle: 'alex',
			home_team_id: ENG,
			owner: true,
			version: 1
		}
	]
};

describe('createHttpTeamsApi', () => {
	it('lists teams with live project roles and member counts, and builds team detail path', async () => {
		const http = {
			read: vi.fn().mockResolvedValue({
				status: 200,
				body: {
					result: {
						projects: [
							{
								id: PROJ,
								key: 'P-1',
								name: 'Checkout',
								owner_team_id: ENG,
								contributing_team_ids: [ROOT],
								archived: false
							}
						]
					}
				}
			}),
			submit: vi.fn()
		};
		const orgApi = {
			view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }),
			command: vi.fn()
		};
		const api = createHttpTeamsApi({ http, orgApi, newEntityId: () => ADA });

		const rows = await api.list(ORG);
		expect(rows).toHaveLength(2);
		const eng = rows.find((t) => t.id === ENG)!;
		expect(eng.people).toBe(1);
		expect(eng.lead).toEqual({ id: OWNER, name: 'Alex Owner' });
		expect(eng.projects).toEqual([{ id: PROJ, name: 'Checkout', role: 'owner' }]);

		const detail = await api.team(ORG, ENG);
		expect(detail.path.map((p) => p.name)).toEqual(['Acme']);
		expect(detail.members).toHaveLength(1);
		expect(detail.members[0].lead).toBe(true);

		const teamProjects = await api.projects(ORG, ROOT);
		expect(teamProjects).toEqual([
			{
				id: PROJ,
				name: 'Checkout',
				role: 'contributor',
				key: 'P-1',
				ownerTeam: { id: ENG, name: 'Engineering', ref: '' },
				openWork: 0
			}
		]);
	});

	it('adds a person with explicit handle/home team, sets optional manager, and tags handle conflicts', async () => {
		const command = vi.fn().mockResolvedValue({ status: 'committed' });
		const view = vi.fn().mockResolvedValue({
			snapshot: {
				...SNAPSHOT,
				members: [
					...SNAPSHOT.members,
					{
						id: ADA,
						kind: 'person',
						name: 'Ada Lovelace',
						handle: 'ada',
						home_team_id: ENG,
						version: 1
					}
				]
			},
			sample: false
		});
		const api = createHttpTeamsApi({
			http: { read: vi.fn(), submit: vi.fn() },
			orgApi: { view, command },
			newEntityId: () => ADA
		});

		const added = await api.addPerson(ORG, {
			name: 'Ada Lovelace',
			handle: 'ada',
			homeTeamId: ENG,
			contact: 'ada@acme.test',
			reportsToId: OWNER
		});
		expect(added).toEqual({ memberId: ADA });
		expect(command).toHaveBeenNthCalledWith(1, ORG, {
			method: 'member.add_person',
			params: {
				member_id: ADA,
				name: 'Ada Lovelace',
				handle: 'ada',
				home_team_id: ENG,
				contact: 'ada@acme.test'
			}
		});
		expect(command).toHaveBeenNthCalledWith(2, ORG, {
			method: 'member.set_reports_to',
			params: { member_id: ADA, expected_version: 1, reports_to_member_id: OWNER }
		});

		command.mockRejectedValueOnce({
			source: 'api',
			code: 'already_exists',
			message: 'Handle @ada is already taken.'
		});
		await expect(
			api.addPerson(ORG, { name: 'Ada Two', handle: 'ada', homeTeamId: ENG })
		).rejects.toMatchObject({ code: 'already_exists', field: 'handle' });

		command.mockRejectedValueOnce({
			source: 'api',
			code: 'invalid_name',
			message: 'Handle may contain only lowercase letters, digits and hyphens.'
		});
		await expect(
			api.addPerson(ORG, { name: 'Ada Bad', handle: 'BAD!', homeTeamId: ENG })
		).rejects.toMatchObject({ code: 'invalid_name', field: 'handle' });
	});

	it('deletes a team using its current snapshot version', async () => {
		const command = vi.fn().mockResolvedValue({ status: 'committed' });
		const api = createHttpTeamsApi({
			http: { read: vi.fn(), submit: vi.fn() },
			orgApi: {
				view: vi.fn().mockResolvedValue({ snapshot: SNAPSHOT, sample: false }),
				command
			}
		});
		await api.delete(ORG, ENG);
		expect(command).toHaveBeenCalledWith(ORG, {
			method: 'team.delete',
			params: { team_id: ENG, expected_version: 2 }
		});
	});
});
