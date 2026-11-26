import type { AppError } from '$lib/proto';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { readResponse, readSession } from '$lib/platform/http/wire';
import type { PreferencesApi } from './api';

/**
 * Production HTTP adapter for account preferences backed by `GET /api/v1/session`
 * and `POST /api/v1/account/preferences`.
 */
export function createHttpPreferencesApi(deps: {
	http: GatewayHttpTransport;
	sessionTimeZone?: () => string | undefined;
	onSaved?: () => Promise<void> | void;
}): PreferencesApi {
	return Object.freeze<PreferencesApi>({
		async timeZone(): Promise<{ saved: string | null }> {
			const cached = deps.sessionTimeZone?.();
			if (cached) return { saved: cached };
			try {
				const reply = await deps.http.read('/api/v1/session');
				if (reply.status !== 200) return { saved: null };
				const env = readResponse(reply.body, readSession);
				if ('result' in env) {
					return { saved: env.result.account.time_zone ?? null };
				}
			} catch {
				// Fall back to browser-detected time zone.
			}
			return { saved: null };
		},

		async saveTimeZone(_organizationId: string, zone: string): Promise<void> {
			let reply: GatewayReply;
			try {
				reply = await deps.http.submit('/api/v1/account/preferences', {
					time_zone: zone
				});
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Preferences could not be reached.'
				} satisfies AppError;
			}
			unwrapGatewayResult(reply, 'Time zone preference could not be saved.');
			await deps.onSaved?.();
		}
	});
}
