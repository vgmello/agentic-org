import { describe, expect, it } from 'vitest';
import { settingsApi } from '$lib/modules/settings/api';
import { accessApi } from './api';

// One contract per capability (architecture rules): the sign-in actions belong to the members module, which the member
// profile and this page both call through its SignInAccessDialogs. Neither this module nor settings keeps a copy, so
// the web API gets one adapter for them.
describe('the access API', () => {
	it('only reads the overview; it has no sign-in actions of its own', () => {
		expect(Object.keys(accessApi)).toEqual(['overview']);
	});

	it('leaves no Members and access calls in the settings API', () => {
		for (const call of ['access', 'issueLink', 'removeAccess'])
			expect(Object.keys(settingsApi)).not.toContain(call);
	});
});
