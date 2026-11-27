import { describe, expect, it } from 'vitest';
import { areaCapability } from './capabilities';

const VALID_ROLE_ID = '0193a5c0-0000-7000-8000-000000000040';

describe('production shell capabilities', () => {
	it('enables session, onboarding, security, U1 org/office/project, B7 home/inbox/chat, B4/B5 work, and FC-108 roles routes', () => {
		for (const path of [
			'/',
			'/sign-in',
			'/register',
			'/set-password',
			'/welcome',
			'/create',
			'/security',
			'/approve',
			'/cli-approve',
			'/home',
			'/inbox',
			'/work',
			'/work/T-A1',
			'/chat',
			'/activity',
			'/insights',
			'/search',
			'/preferences',
			'/settings',
			'/settings/offices',
			'/offices',
			'/settings/agents',
			'/settings/agents/0193a5c0-0000-7000-8000-000000000002',
			'/settings/roles',
			'/settings/roles/',
			`/settings/roles/${VALID_ROLE_ID}`,
			`/settings/roles/${VALID_ROLE_ID}/`,
			'/settings/processes',
			'/settings/processes/development',
			'/settings/processes/documents/new',
			'/settings/processes/documents/doc-1',
			'/processes',
			'/settings/access',
			'/settings/security',
			'/settings/costs',
			'/settings/updates',
			'/teams',
			'/teams/hire',
			'/teams/hires',
			'/teams/members',
			'/agents',
			'/teams/org-chart',
			'/org-chart',
			'/teams/0193a5c0-0000-7000-8000-000000000001',
			'/teams/0193a5c0-0000-7000-8000-000000000001/members',
			'/teams/0193a5c0-0000-7000-8000-000000000001/work',
			'/teams/0193a5c0-0000-7000-8000-000000000001/projects',
			'/teams/0193a5c0-0000-7000-8000-000000000001/activity',
			'/teams/0193a5c0-0000-7000-8000-000000000001/conversations',
			'/teams/0193a5c0-0000-7000-8000-000000000001/processes',
			'/members/0193a5c0-0000-7000-8000-000000000002',
			'/members/0193a5c0-0000-7000-8000-000000000002/work',
			'/members/0193a5c0-0000-7000-8000-000000000002/conversations',
			'/members/0193a5c0-0000-7000-8000-000000000002/activity',
			'/members/0193a5c0-0000-7000-8000-000000000002/role',
			'/members/0193a5c0-0000-7000-8000-000000000002/cost',
			'/members/0193a5c0-0000-7000-8000-000000000002/processes',
			'/members/0193a5c0-0000-7000-8000-000000000002/runs',
			'/members/0193a5c0-0000-7000-8000-000000000002/runs/run-1',
			'/members/0193a5c0-0000-7000-8000-000000000002/notes',
			'/projects',
			'/projects/0193a5c0-0000-7000-8000-000000000003',
			'/projects/0193a5c0-0000-7000-8000-000000000003/work',
			'/projects/0193a5c0-0000-7000-8000-000000000003/activity',
			'/projects/0193a5c0-0000-7000-8000-000000000003/insights',
			'/projects/0193a5c0-0000-7000-8000-000000000003/conversations',
			'/projects/0193a5c0-0000-7000-8000-000000000003/team',
			'/projects/0193a5c0-0000-7000-8000-000000000003/settings'
		])
			expect(areaCapability(path), path).toEqual({ available: true });
	});

	it('keeps unbuilt areas, reserved/malformed role segments, and subroutes unavailable until their adapters land', () => {
		for (const path of [
			'/create/x',
			'/security/x',
			'/approve/x',
			'/cli-approve/x',
			'/work/T-A1/extra',
			'/settings/roles/update',
			'/settings/roles/update/',
			'/settings/roles/role-swe',
			`/settings/roles/${VALID_ROLE_ID.toUpperCase()}`,
			`/settings/roles/${VALID_ROLE_ID}/extra`,
			'/settings/roles//',
			'/projects/p1/docs',
			'/projects/p1/features',
			'/projects/p1/releases',
			'/sessions',
			'/future-product-area'
		])
			expect(areaCapability(path), path).toMatchObject({
				available: false,
				reason: 'unavailable'
			});
	});
});
