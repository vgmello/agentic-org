import { describe, expect, it } from 'vitest';
import { chatApi, sender } from './api';

describe('production chatApi fails closed until backend messaging routes land', () => {
	it('rejects snapshot, command, and status with unavailable error', async () => {
		await expect(chatApi.snapshot('org-1')).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		await expect(
			chatApi.command('org-1', {
				method: 'msg.send',
				change_id: 'chg-1',
				conversation_id: 'c-general',
				kind: 'msg',
				body: 'hello'
			})
		).rejects.toMatchObject({ source: 'api', code: 'unavailable' });
		await expect(chatApi.status('org-1', 'chg-1')).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		const send = sender('org-1');
		await expect(
			send({
				method: 'msg.send',
				conversation_id: 'c-general',
				kind: 'msg',
				body: 'hello'
			})
		).rejects.toMatchObject({ source: 'api', code: 'unavailable' });
	});

	it('subscribe returns a no-op unsubscribe handle', () => {
		const stop = chatApi.subscribe('org-1', () => undefined);
		expect(() => stop()).not.toThrow();
	});
});
