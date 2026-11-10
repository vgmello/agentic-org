import { unavailableFeature } from '../feature-factory';
import { UNAVAILABLE_MESSAGE } from '../unavailable-api';

// Enable areas only as real gateway adapters land, including any future product routes.
const unavailable = unavailableFeature(UNAVAILABLE_MESSAGE);
export function areaCapability(_pathname: string) {
	return unavailable();
}
