import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import shapes from '../../../test-vectors/command-shapes.json';
import type { Connection } from '$lib/proto';
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

	it('tauri.ts invokes exactly the commands build.rs declares', () => {
		const names = (text: string) => [...text.matchAll(/['"]([a-z]+(?:_[a-z]+)+)['"]/g)].map((m) => m[1]).sort();
		const ui = names(readFileSync('src/lib/office/tauri.ts', 'utf8'));
		const build = names(readFileSync('src-tauri/build.rs', 'utf8'));
		expect(ui).toEqual(build);
	});
});
