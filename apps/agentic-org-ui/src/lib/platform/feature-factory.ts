/** A local composition seam, separate from B1-02's server capability wire DTOs. */
export type FeatureCapability<Api extends object> =
	| { readonly available: true; readonly api: Api }
	| { readonly available: false; readonly reason: 'unavailable'; readonly message: string };

export type FeatureFactory<Api extends object> = () => FeatureCapability<Api>;

/** Production defaults cannot silently create sample data when a real implementation is absent. */
export function unavailableFeature<Api extends object>(
	message = 'This feature is not available yet.'
): FeatureFactory<Api> {
	const capability = Object.freeze({ available: false as const, reason: 'unavailable' as const, message });
	return () => capability;
}
