import { describe, expect, it } from 'vitest';
import type { OrgSnapshot } from '$lib/proto';
import { withPlacements } from './placements';

const snapshot = { position: {}, teams: [], memberships: [], members: [{ id: 'a', kind: 'agent', name: 'A', office_id: 'hq', version: 1 }, { id: 'b', kind: 'agent', name: 'B', office_id: 'hq', version: 1 }] } as unknown as OrgSnapshot;

describe('withPlacements', () => {
	it('moves an agent, or unplaces it', () => {
		const next = withPlacements(snapshot, { a: 'studio', b: null });
		expect(next.members[0].office_id).toBe('studio');
		expect(next.members[1].office_id).toBeUndefined();
	});
	it('keeps the snapshot when nothing is laid over it', () => {
		expect(withPlacements(snapshot, {})).toBe(snapshot);
	});
});
