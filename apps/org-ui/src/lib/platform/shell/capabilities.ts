import { UNAVAILABLE_MESSAGE } from '../unavailable-api';

// Enable areas only as real gateway adapters land, including any future product routes.
const unavailable = Object.freeze({ available: false as const, reason: 'unavailable' as const, message: UNAVAILABLE_MESSAGE });
export function areaCapability(_pathname: string) {
	return unavailable;
}
