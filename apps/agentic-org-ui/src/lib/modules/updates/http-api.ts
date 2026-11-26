import type { AppError } from '$lib/proto';
import { type GatewayHttpTransport } from '$lib/platform/http/gateway-errors';
import { readInstance, readOfficeList, readResponse } from '$lib/platform/http/wire';
import type { UpdatesApi } from './api';
import type { OfficeVersion, UpdatesOverview } from './types';

/**
 * Production HTTP adapter for Organization settings › Updates backed by `/api/v1/instance`
 * and `/api/v1/organizations/{org}/offices`.
 */
export function createHttpUpdatesApi(deps: { http: GatewayHttpTransport }): UpdatesApi {
	return Object.freeze<UpdatesApi>({
		async overview(organizationId: string): Promise<UpdatesOverview> {
			const [instanceReply, officesReply] = await Promise.all([
				deps.http.read('/api/v1/instance').catch(() => undefined),
				deps.http
					.read(`/api/v1/organizations/${encodeURIComponent(organizationId)}/offices`)
					.catch(() => undefined)
			]);

			if (!instanceReply || instanceReply.status !== 200) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Instance release information could not be loaded.'
				} satisfies AppError;
			}

			const instanceEnv = readResponse(instanceReply.body, readInstance);
			if ('error' in instanceEnv) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: instanceEnv.error.message || 'Instance release information could not be loaded.'
				} satisfies AppError;
			}

			const instanceName = instanceEnv.result.name || 'default';
			const serverVersion = instanceEnv.result.server_version;

			let offices: OfficeVersion[] = [];
			if (officesReply && officesReply.status === 200) {
				const officesEnv = readResponse(officesReply.body, readOfficeList);
				if ('result' in officesEnv) {
					offices = officesEnv.result.offices.map((office) => {
						const role = office.role === 'hq' ? 'hq' : 'satellite';
						const holdsBack =
							office.status === 'update_needed' || office.status_reason === 'unsupported_version';
						return {
							id: office.id,
							name: office.name,
							role,
							host: office.host,
							instance: role === 'hq' ? instanceName : null,
							version: role === 'hq' ? serverVersion : null,
							holdsBack
						};
					});
				}
			}

			if (offices.length === 0) {
				offices = [
					{
						id: instanceEnv.result.instance_id,
						name: 'HQ',
						role: 'hq',
						host: instanceName,
						instance: instanceName,
						version: serverVersion,
						holdsBack: false
					}
				];
			}

			return {
				instance: { name: instanceName, version: serverVersion },
				check: { state: 'unknown' },
				offices
			};
		}
	});
}
