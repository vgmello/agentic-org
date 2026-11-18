import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { WireDecodeError } from '../http/wire';
import {
	readCreateRefusal,
	readErrorBody,
	readOrganizationView,
	readOrganizationsList,
	readOutcomeBody
} from './readers';
import { OP, OP2, office, organization } from './testing';

type Case = { name: string; category: string; wire: unknown; expect: Record<string, unknown> };
const shared: { cases: Case[] } = JSON.parse(
	readFileSync(
		new URL(
			'../../../../../../crates/org-proto/test-vectors/v2/gateway-organizations.json',
			import.meta.url
		),
		'utf8'
	)
);
const category = (name: string) => shared.cases.filter((c) => c.category === name);

describe('shared organization vectors', () => {
	it('covers every organization and list case of the shared file', () => {
		expect(category('organization')).toHaveLength(3);
		expect(category('list')).toHaveLength(1);
	});
	for (const c of category('organization'))
		it(c.name, () => {
			if (c.expect.valid === false)
				expect(() => readOrganizationView(c.wire)).toThrow(WireDecodeError);
			else expect(readOrganizationView(c.wire)).toEqual(c.expect.normalized);
		});
	for (const c of category('list'))
		it(c.name, () => expect(readOrganizationsList(c.wire)).toEqual(c.expect.normalized));
});

describe('organization view', () => {
	it('ignores unknown fields and keeps only the view', () => {
		expect(readOrganizationView({ ...organization, future: { a: 1 }, recovery_code: 'x' })).toEqual(
			{
				...organization
			}
		);
	});
	it.each(['id', 'slug', 'name', 'created_at', 'employee_id', 'recovery', 'offices'])(
		'refuses a view without %s',
		(key) => {
			const { [key]: _missing, ...rest } = organization as Record<string, unknown>;
			expect(() => readOrganizationView(rest)).toThrow(WireDecodeError);
			expect(() => readOrganizationView({ ...organization, [key]: null })).toThrow(WireDecodeError);
		}
	);
	it.each(['a b', 'org/1', '../x', 'é', 'a'.repeat(65), 'a_b', '<b>'])(
		'refuses a view whose id (%j) the selection would not keep',
		(id) => expect(() => readOrganizationView({ ...organization, id })).toThrow(WireDecodeError)
	);
	it.each(['org-1', 'A', 'a'.repeat(64), organization.id])('reads a view with the id %j', (id) =>
		expect(readOrganizationView({ ...organization, id }).id).toBe(id)
	);
	it('puts HQ first and reads an unknown recovery state as unknown', () => {
		const satellite = { ...office, id: 'sat', role: 'satellite', name: 'Lab' };
		const view = readOrganizationView({
			...organization,
			offices: [satellite, office],
			recovery: 'later'
		});
		expect(view.offices.map((o) => o.role)).toEqual(['hq', 'satellite']);
		expect(view.recovery).toBe('unknown');
	});
	it.each([null, [], 'text', 1])('refuses %j', (value) => {
		expect(() => readOrganizationView(value)).toThrow(WireDecodeError);
	});
});

describe('organization list', () => {
	it('reads a list without creating as none in flight', () => {
		expect(readOrganizationsList({ organizations: [] })).toEqual({
			organizations: [],
			creating: []
		});
	});
	it('refuses a list without organizations and a create without a ULID', () => {
		expect(() => readOrganizationsList({ creating: [] })).toThrow(WireDecodeError);
		expect(() =>
			readOrganizationsList({
				organizations: [],
				creating: [{ operation_id: 'nope', name: 'A', slug: 'a' }]
			})
		).toThrow(WireDecodeError);
		expect(() =>
			readOrganizationsList({ organizations: [], creating: [{ operation_id: OP, name: 'A' }] })
		).toThrow(WireDecodeError);
	});
});

describe('refusals and outcomes', () => {
	it('keeps machine fields only, and only the form fields as a field', () => {
		expect(
			readCreateRefusal({
				code: 'already_exists',
				message: 'secret text',
				field: 'slug',
				rule: 'taken'
			})
		).toEqual({ code: 'already_exists', field: 'slug', rule: 'taken' });
		expect(
			readCreateRefusal({ code: 'invalid_request', field: 'operation_id', rule: 'format' })
		).toEqual({
			code: 'invalid_request',
			rule: 'format'
		});
		expect(readErrorBody({ error: { code: 'expired' } })).toEqual({ code: 'expired' });
		expect(() => readCreateRefusal({ message: 'no code' })).toThrow(WireDecodeError);
		expect(() => readErrorBody({})).toThrow(WireDecodeError);
	});
	it('reads each outcome of the asked operation', () => {
		expect(readOutcomeBody({ operation_id: OP, outcome: 'accepted' }, OP)).toEqual({
			outcome: 'accepted'
		});
		expect(readOutcomeBody({ operation_id: OP, outcome: 'unknown' }, OP)).toEqual({
			outcome: 'unknown'
		});
		expect(
			readOutcomeBody({ operation_id: OP, outcome: 'completed', result: organization }, OP)
		).toEqual({ outcome: 'completed', organization });
		expect(
			readOutcomeBody(
				{ operation_id: OP, outcome: 'rejected', error: { code: 'not_authorized' } },
				OP
			)
		).toEqual({ outcome: 'rejected', error: { code: 'not_authorized' } });
	});
	it('refuses an answer about another operation, a completed without a view and a new outcome', () => {
		expect(() => readOutcomeBody({ operation_id: OP2, outcome: 'accepted' }, OP)).toThrow(
			WireDecodeError
		);
		expect(() => readOutcomeBody({ operation_id: OP, outcome: 'completed' }, OP)).toThrow(
			WireDecodeError
		);
		expect(() => readOutcomeBody({ operation_id: OP, outcome: 'paused' }, OP)).toThrow(
			WireDecodeError
		);
	});
});
