import { expect, it } from 'vitest';
import { ROLES_URL, roleHref } from './urls';

it('links a role by its stable ID, escaped', () => {
	expect(ROLES_URL).toBe('/settings/roles');
	expect(roleHref('role-swe')).toBe('/settings/roles/role-swe');
	expect(roleHref('a/b c')).toBe('/settings/roles/a%2Fb%20c');
});
