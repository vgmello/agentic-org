// Mock for src/lib/modules/preferences/api.ts under `bun run dev:mock`: the saved zone lives in memory.
import type { PreferencesApi } from '../../../src/lib/modules/preferences/api';
import saved from '../data/preferences/account.json' with { type: 'json' };

let zone: string | null = saved.time_zone;

export const preferencesApi: PreferencesApi = {
	async timeZone() {
		return { saved: zone };
	},
	async saveTimeZone(_organizationId, next) {
		try {
			new Intl.DateTimeFormat('en-GB', { timeZone: next });
		} catch {
			throw new Error(`${next} is not a time zone the office knows.`);
		}
		zone = next;
	}
};
