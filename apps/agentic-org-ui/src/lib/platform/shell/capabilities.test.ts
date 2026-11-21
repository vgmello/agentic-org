import { describe, expect, it } from 'vitest';
import { areaCapability } from './capabilities';

describe('production shell capabilities', () => {
	it('enables session, onboarding, security, U1 org/office/project, and B7 home/inbox/chat routes', () => {
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
			'/chat',
			'/settings/offices',
			'/offices',
			'/settings/access',
			'/teams',
			'/teams/members',
			'/agents',
			'/teams/org-chart',
			'/org-chart',
			'/teams/0193a5c0-0000-7000-8000-000000000001',
			'/teams/0193a5c0-0000-7000-8000-000000000001/projects',
			'/members/0193a5c0-0000-7000-8000-000000000002',
			'/members/0193a5c0-0000-7000-8000-000000000002/role',
			'/projects',
			'/projects/0193a5c0-0000-7000-8000-000000000003',
			'/projects/0193a5c0-0000-7000-8000-000000000003/team',
			'/projects/0193a5c0-0000-7000-8000-000000000003/settings'
		])
			expect(areaCapability(path), path).toEqual({ available: true });
	});

	it('keeps unbuilt areas and subroutes unavailable until their adapters land', () => {
		for (const path of [
			'/create/x',
			'/security/x',
			'/approve/x',
			'/cli-approve/x',
			'/work',
			'/activity',
			'/settings',
			'/settings/costs',
			'/settings/agents',
			'/settings/processes',
			'/teams/hire',
			'/teams/hires',
			'/teams/t1/processes',
			'/members/m1/runs',
			'/members/m1/cost',
			'/insights',
			'/projects/p1/docs',
			'/projects/p1/features',
			'/projects/p1/releases',
			'/projects/p1/insights',
			'/projects/p1/work',
			'/search',
			'/processes',
			'/sessions',
			'/future-product-area'
		])
			expect(areaCapability(path), path).toMatchObject({
				available: false,
				reason: 'unavailable'
			});
	});
});
