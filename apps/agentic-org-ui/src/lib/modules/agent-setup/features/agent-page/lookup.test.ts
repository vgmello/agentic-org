import { describe, expect, it } from 'vitest';
import type { AgentSetupDetail } from '../../types';
import { latestOf, lookUpAgent } from './lookup';

const detail = { memberId: 'kai' } as AgentSetupDetail;

describe('lookUpAgent', () => {
	it('answers the agent, or not found as its own state', async () => {
		expect(await lookUpAgent(async () => detail)).toEqual({ found: true, detail });
		expect(
			await lookUpAgent(async () => {
				throw { source: 'api', code: 'not_found', message: 'Gone.' };
			})
		).toEqual({ found: false, message: 'Gone.' });
	});

	it('keeps any other failure an error', async () => {
		await expect(
			lookUpAgent(async () => {
				throw { source: 'api', code: 'storage_error', message: 'Broken.' };
			})
		).rejects.toMatchObject({ code: 'storage_error' });
	});
});

it('reads the newer record a conflict carries, and nothing else', () => {
	expect(latestOf({ code: 'conflict', latest: detail })).toBe(detail);
	expect(latestOf({ code: 'conflict' })).toBeUndefined();
	expect(latestOf({ latest: 'x' })).toBeUndefined();
	expect(latestOf(undefined)).toBeUndefined();
});
