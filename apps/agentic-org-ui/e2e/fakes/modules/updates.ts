// Mock for src/lib/modules/updates/api.ts under `bun run dev:mock`. The offices come from the Offices mock's seed (same
// IDs, names and hosts as Organization settings › Offices); their releases and the update check are JSON in
// e2e/fakes/data/updates. Read only: nothing here changes.
// Scenarios (`?mock=` on the page's URL, several joined with commas):
//   slow · fails (the read fails) · fails-once (the first read fails, a Retry succeeds) · empty (HQ alone, nothing
//   reported) · unavailable · no-instance (HQ doesn't know the other offices' instance names)
//   current · failed · offline · unknown (the update check's state; default: an update is available)
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { UpdatesApi } from '../../../src/lib/modules/updates/api';
import type {
	OfficeVersion,
	UpdateCheck,
	UpdatesOverview
} from '../../../src/lib/modules/updates/types';
import { mockMode } from '../mock-mode';
import data from '../data/updates/overview.json' with { type: 'json' };
import offices from '../data/offices/seed.json' with { type: 'json' };

const on = (name: string) => mockMode().split(',').includes(name);
let failedOnce = false;
const ago = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();

function check(): UpdateCheck {
	const name = (['current', 'failed', 'offline', 'unknown'] as const).find(on) ?? 'available';
	const { checkedMinutesAgo, ...rest } = data.check[on('empty') ? 'unknown' : name] as {
		checkedMinutesAgo?: number;
	} & Record<string, unknown>;
	return {
		...rest,
		...(checkedMinutesAgo === undefined ? {} : { checkedAt: ago(checkedMinutesAgo) })
	} as UpdateCheck;
}

export const updatesApi: UpdatesApi = {
	async overview(organizationId) {
		await new Promise((r) => setTimeout(r, on('slow') ? 4000 : 250));
		if (on('unavailable')) await unavailableCall();
		const failOnce = on('fails-once') && !failedOnce;
		if (failOnce) failedOnce = true;
		if (on('fails') || failOnce)
			throw Object.assign(new Error("HQ didn't answer. Check that it's running, then try again."), {
				source: 'app',
				code: 'unreachable',
				message: "HQ didn't answer. Check that it's running, then try again."
			});
		const hq: OfficeVersion = {
			id: `office-${organizationId}`,
			name: data.hq.name,
			role: 'hq',
			host: data.hq.host,
			instance: data.instance.name,
			version: data.instance.version,
			holdsBack: false
		};
		const versions = data.offices as Record<
			string,
			{ instance: string | null; version: string | null; holdsBack: boolean }
		>;
		const satellites: OfficeVersion[] = on('empty')
			? []
			: offices.satellites.map((s) => ({
					id: `office-${organizationId}-${s.key}`,
					name: s.name,
					role: 'satellite',
					host: s.host,
					instance: on('no-instance') ? null : (versions[s.key]?.instance ?? null),
					version: versions[s.key]?.version ?? null,
					holdsBack: versions[s.key]?.holdsBack ?? false
				}));
		const overview: UpdatesOverview = {
			instance: { ...data.instance },
			check: check(),
			offices: [hq, ...satellites]
		};
		return structuredClone(overview);
	}
};
