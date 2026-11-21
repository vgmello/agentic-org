import { describe, expect, it } from 'vitest';
import { copilotApi, copilotFeatures } from '$lib/modules/copilot/api';
import { chatApi, sender as chatSender } from '$lib/modules/messaging/api';
import { officeApi, toAppError } from '$lib/modules/offices/office-api';
import { orgApi } from '$lib/modules/organization/api';
import { sender, workApi } from '$lib/modules/work';

describe('production APIs before concrete gateway adapters exist', () => {
	it('refuses office reads and writes rather than creating sample connections or starting services', async () => {
		for (const result of [
			officeApi.connections(),
			officeApi.ensureRunning(),
			officeApi.createOrganization('Acme'),
			officeApi.onOfficeStatus(() => {})
		])
			await expect(result).rejects.toMatchObject({ source: 'api', code: 'unavailable' });
		expect('loginItem' in officeApi).toBe(false);
		expect('openLoginSettings' in officeApi).toBe(false);
	});
	it('refuses organization reads, writes and subscriptions without a browser reducer fallback', async () => {
		for (const result of [
			orgApi.view('org'),
			orgApi.command('org', { method: 'team.create', params: { team_id: 'team', name: 'Ops' } }),
			orgApi.onFeed(() => {})
		])
			await expect(result).rejects.toMatchObject({ source: 'api', code: 'unavailable' });
	});
	it('refuses work reads and writes rather than writing sample work to storage', async () => {
		await expect(workApi.snapshot('org')).rejects.toMatchObject({ code: 'unavailable' });
		await expect(
			sender('org')({
				method: 'task.create',
				item_id: '01ARZ3NDEKTSV4RRFFQ69G5FAV',
				project_id: 'project',
				title: 'Task',
				priority: 'normal'
			})
		).rejects.toMatchObject({ code: 'unavailable' });
		expect(workApi.requests).toBe(false);
	});
	it('refuses chat reads and writes without importing sample conversations or mock engines', async () => {
		await expect(chatApi.snapshot('org')).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		await expect(
			chatSender('org')({ method: 'dm.open', members: ['a', 'b'] })
		).rejects.toMatchObject({ source: 'api', code: 'unavailable' });
		const unsubscribe = chatApi.subscribe('org', () => {});
		expect(() => unsubscribe()).not.toThrow();
	});
	it('refuses copilot calls and hides smart features in production without a scripted mock', async () => {
		expect(copilotApi.sample).toBe(false);
		await expect(copilotApi.features()).rejects.toMatchObject({
			source: 'api',
			code: 'unavailable'
		});
		await expect(
			copilotApi.draft('bug.from_sentence', { sentence: 'broken' })
		).rejects.toMatchObject({ source: 'api', code: 'unavailable' });
		expect(await copilotFeatures(copilotApi)).toEqual([]);
	});
	it('keeps the missing-adapter error explicit through the existing browser error boundary', async () => {
		const error = await officeApi.connections().catch(toAppError);
		expect(error).toMatchObject({
			source: 'api',
			code: 'unavailable',
			message: 'This area is not available yet.'
		});
	});
	it('keeps the message of an Error whose code this build does not know', () => {
		const error = toAppError(
			Object.assign(new Error('Office stopped'), { source: 'api', code: 'from_a_newer_office' })
		);
		expect(error).toEqual({ source: 'api', code: 'unknown', message: 'Office stopped' });
	});
});
