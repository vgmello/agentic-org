import { expect, it } from 'vitest';
import { unavailableFeature, type FeatureFactory } from './feature-factory';

it('has an explicit immutable unavailable default with no API or sample implementation', () => {
	const factory = unavailableFeature<{ load(): Promise<string[]> }>();
	const capability = factory();
	expect(capability).toEqual({ available: false, reason: 'unavailable', message: 'This feature is not available yet.' });
	expect(capability).not.toHaveProperty('api');
	expect(Object.isFrozen(capability)).toBe(true);
	expect(factory()).toBe(capability);
});

it('allows explicit implementation selection through the same small factory contract', async () => {
	const api = { load: async () => ['server result'] };
	const factory: FeatureFactory<typeof api> = () => ({ available: true, api });
	const capability = factory();
	if (!capability.available) throw new Error('Expected real implementation');
	expect(await capability.api.load()).toEqual(['server result']);
	expect(unavailableFeature('The operation is not offered by this server.')()).toMatchObject({ available: false, message: 'The operation is not offered by this server.' });
});
