import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import shapes from '../../../test-vectors/command-shapes.json';
import type { CommandAccepted, Connection, OrgView } from '$lib/proto';
import { toAppError } from './api';

// The same fixture the Rust side serializes its command results against (src-tauri/src/lib.rs).
describe('what the Tauri commands return', () => {
	it('an error is read as an AppError unchanged', () => {
		expect(toAppError(shapes.command_error)).toEqual(shapes.command_error);
	});

	it('a connection has the keys proto.ts declares', () => {
		const connection = shapes.connection as Connection;
		expect(Object.keys(connection).sort()).toEqual(['office', 'organization']);
		expect(Object.keys(connection.organization).sort()).toEqual(['created_at', 'id', 'name', 'offices']);
		const officeKeys = ['host', 'id', 'name', 'organization_id', 'role', 'status'];
		expect(Object.keys(connection.office).sort()).toEqual(officeKeys);
		expect(Object.keys(connection.organization.offices[0]).sort()).toEqual(officeKeys);
	});

	it('an organization view has the keys proto.ts declares, and an agent is placed in an office', () => {
		const view = shapes.org_view as OrgView;
		expect(Object.keys(view).sort()).toEqual(['sample', 'snapshot']);
		expect(Object.keys(view.snapshot).sort()).toEqual(['members', 'memberships', 'position', 'project_teams', 'projects', 'teams']);
		expect(view.snapshot.members.find((m) => m.kind === 'agent')).toMatchObject({ office_id: 'office-1', reports_to_member_id: 'you' });
		expect(view.snapshot.members.find((m) => m.owner)).not.toHaveProperty('office_id');
		const agent = view.snapshot.members.find((m) => m.kind === 'agent')!;
		expect(Object.keys(agent).sort()).toEqual(['agent', 'id', 'kind', 'name', 'office_id', 'reports_to_member_id', 'version']);
		expect(Object.keys(agent.agent!).sort()).toEqual(['messaging_name', 'role', 'run_mode', 'runtime', 'status', 'title']);
		expect(agent.agent).toMatchObject({ runtime: 'claude_code', status: 'active', run_mode: 'on_call' });
	});

	it('a command answer has the keys proto.ts declares', () => {
		const accepted = shapes.command_accepted as CommandAccepted;
		expect(Object.keys(accepted).sort()).toEqual(['position', 'status']);
		expect(accepted.status).toBe('committed');
	});

	it('tauri.ts invokes exactly the commands build.rs declares', () => {
		const names = (text: string) => [...text.matchAll(/['"]([a-z]+(?:_[a-z]+)+)['"]/g)].map((m) => m[1]).sort();
		const ui = [...names(readFileSync('src/lib/office/tauri.ts', 'utf8')), ...names(readFileSync('src/lib/org/tauri.ts', 'utf8'))].sort();
		const build = names(readFileSync('src-tauri/build.rs', 'utf8'));
		expect(ui).toEqual(build);
	});
});
