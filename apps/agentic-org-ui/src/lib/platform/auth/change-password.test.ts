import { describe, expect, it, vi } from 'vitest';
import type { JsonHttpRequest } from '../http/json-http';
import {
	CURRENT_PASSWORD_WRONG_COPY,
	describeChangePasswordFailure,
	UNREACHABLE_COPY
} from './credential-copy';
import { createSessionController } from './session-controller';
import { SessionRequestError } from './session-gateway';
import type { SessionRecord } from './session-record';

const proof = 'a'.repeat(64);
const rotated = 'b'.repeat(64);
const record: SessionRecord = {
	instance_id: 'hq',
	account_id: 'alex-id',
	login: 'alex',
	proof,
	rotated_at_ms: 100
};
const view = {
	instance_id: 'hq',
	account: { id: 'alex-id', login: 'alex' },
	admin: false,
	organizations: [],
	server_time_ms: 100,
	idle_expires_at: 'sooner'
};

function setup(initial?: SessionRecord) {
	let stored = initial;
	const store = {
		read: vi.fn(async () => stored),
		write: vi.fn(async (value: SessionRecord) => {
			stored = value;
		}),
		clear: vi.fn(async () => {
			stored = undefined;
		})
	};
	const changePassword = vi.fn(async () => ({}));
	const gateway = {
		version: vi.fn(async () => ({ api_version: 1, contract_revision: 2 })),
		get: vi.fn(async () => view),
		signIn: vi.fn(async () => ({ ...view, proof: rotated })),
		register: vi.fn(async () => ({ ...view, proof: rotated })),
		refresh: vi.fn(async () => ({ ...view, proof: rotated })),
		signOut: vi.fn(async () => ({})),
		changePassword
	};
	const http = vi.fn<(request: JsonHttpRequest) => Promise<{ status: number; body: unknown }>>(
		async () => ({ status: 200, body: { result: {} } })
	);
	let tail = Promise.resolve<unknown>(undefined);
	const locks = {
		request: ((_name: string, callback: () => unknown) => {
			const run = tail.then(callback);
			tail = run.catch(() => {});
			return run;
		}) as LockManager['request']
	};
	const controller = createSessionController({
		store,
		gateway,
		http,
		locks,
		now: () => 200,
		channel: {
			postMessage: vi.fn(),
			addEventListener: vi.fn() as unknown as BroadcastChannel['addEventListener'],
			removeEventListener: vi.fn() as unknown as BroadcastChannel['removeEventListener']
		},
		changed: vi.fn()
	});
	return { controller, changePassword };
}

describe('change password', () => {
	it('maps 403 wrong current password to exact §4a.2 copy and 503 to unreachable', () => {
		expect(
			describeChangePasswordFailure(
				new SessionRequestError(403, { code: 'invalid_credentials', message: 'wrong password' }),
				1_000
			)
		).toEqual({ kind: 'current_password', message: CURRENT_PASSWORD_WRONG_COPY });

		expect(
			describeChangePasswordFailure(
				new SessionRequestError(400, {
					code: 'invalid_input',
					message: 'Password too short.',
					field: 'password',
					rule: 'too_short'
				}),
				1_000
			)
		).toEqual({ kind: 'new_password', message: 'Use at least 12 characters.' });

		expect(
			describeChangePasswordFailure(
				new SessionRequestError(503, { code: 'unavailable', message: 'Service unavailable.' }),
				1_000
			)
		).toEqual({
			kind: 'general',
			message: UNREACHABLE_COPY
		});
	});

	it('forwards changePassword with caller session proof when signed in', async () => {
		const { controller, changePassword } = setup(record);
		await controller.boot();
		await controller.changePassword({
			current_password: 'old-password-1',
			new_password: 'new-password-2'
		});
		expect(changePassword).toHaveBeenCalledWith(
			{
				current_password: 'old-password-1',
				new_password: 'new-password-2'
			},
			proof,
			undefined
		);
	});

	it('fails closed when signed out', async () => {
		const { controller, changePassword } = setup(undefined);
		await controller.boot();
		await expect(
			controller.changePassword({
				current_password: 'old-password-1',
				new_password: 'new-password-2'
			})
		).rejects.toThrow();
		expect(changePassword).not.toHaveBeenCalled();
	});
});
