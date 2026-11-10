import { describe, expect, it } from 'vitest';
import wire from '../../../../../crates/org-proto/test-vectors/office-wire-shapes.json';
import { API_ERROR_CODES } from '$lib/proto';

// No Tauri wrapper or app-local fixture: this side of the serialization gate survives F1-03.
describe('canonical office wire payloads shared with Rust', () => {
	it('keeps API errors and command outcomes independent of desktop wrappers', () => {
		expect(Object.keys(wire.api_error).sort()).toEqual(['code', 'message']);
		expect(API_ERROR_CODES).toContain(wire.api_error.code);
		expect(Object.keys(wire.command_accepted).sort()).toEqual(['position', 'status']);
		expect(wire.command_accepted.status).toBe('committed');
		expect(wire.command_accepted.position).toEqual({ epoch: 1, seq: 2 });
	});

	it('keeps office and organization payloads without a desktop connection pair', () => {
		expect(Object.keys(wire.office).sort()).toEqual(['host', 'id', 'name', 'organization_id', 'role', 'status']);
		expect(Object.keys(wire.organization).sort()).toEqual(['created_at', 'id', 'name', 'offices']);
		expect(wire.organization.offices).toEqual([wire.office]);
	});

	it('keeps the snapshot, placement and profile contract without the sample flag', () => {
		expect(Object.keys(wire.snapshot).sort()).toEqual(['areas', 'members', 'memberships', 'position', 'project_teams', 'projects', 'teams']);
		const agent = wire.snapshot.members.find((member) => member.kind === 'agent');
		expect(agent).toMatchObject({ office_id: 'office-1', reports_to_member_id: 'you' });
		expect(agent?.agent).toMatchObject({ runtime: 'claude_code', status: 'active', run_mode: 'on_call' });
		expect(wire.snapshot.members.find((member) => member.owner)).not.toHaveProperty('office_id');
	});
});
