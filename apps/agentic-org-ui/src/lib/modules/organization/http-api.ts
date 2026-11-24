import {
	API_ERROR_CODES,
	type ApiErrorCode,
	type AppError,
	type CommandAccepted,
	type OrgSnapshot
} from '$lib/proto';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { formatUuidV7 } from '$lib/platform/http/clock';
import { readOperationId } from '$lib/platform/http/wire';
import { ready, type OrgCommand, type OrgCommandInput } from './commands';
import { normalizeKinds } from './model';
import type { OrgApi } from './api';

type Reply = { status: number; body: unknown };

export interface OrgHttpTransport {
	read(path: string, signal?: AbortSignal): Promise<Reply>;
	submit(path: string, body: unknown, signal?: AbortSignal): Promise<Reply>;
}

const KNOWN_CODES: ReadonlySet<string> = new Set(API_ERROR_CODES);

function asApiCode(code: unknown): ApiErrorCode {
	return typeof code === 'string' && KNOWN_CODES.has(code) ? (code as ApiErrorCode) : 'unknown';
}

function readAppError(
	body: unknown,
	fallbackCode: ApiErrorCode,
	fallbackMessage: string
): AppError {
	const root = body && typeof body === 'object' ? (body as Record<string, unknown>) : undefined;
	const err =
		root && root.error && typeof root.error === 'object'
			? (root.error as Record<string, unknown>)
			: undefined;
	return {
		source: 'api',
		code: err ? asApiCode(err.code) : fallbackCode,
		message: typeof err?.message === 'string' && err.message ? err.message : fallbackMessage
	};
}

function isSnapshot(value: unknown): value is OrgSnapshot {
	if (!value || typeof value !== 'object') return false;
	const s = value as Partial<OrgSnapshot>;
	return (
		!!s.position &&
		typeof s.position === 'object' &&
		typeof s.position.epoch === 'number' &&
		typeof s.position.seq === 'number' &&
		Array.isArray(s.teams) &&
		Array.isArray(s.members)
	);
}

function prepareOrgCommand(input: OrgCommandInput | OrgCommand, newId: () => string): OrgCommand {
	const explicitId = 'command_id' in input.params ? input.params.command_id : undefined;
	let operationId: string;
	if (explicitId !== undefined) {
		try {
			operationId = readOperationId(explicitId);
		} catch {
			throw {
				source: 'api',
				code: 'invalid_request',
				message: 'Operation ID must be a canonical lowercase UUIDv7.'
			} satisfies AppError;
		}
	} else {
		operationId = readOperationId(newId());
	}
	return ready({
		...input,
		params: { ...input.params, command_id: operationId }
	} as OrgCommand);
}

const SUPPORTED_METHODS: ReadonlySet<string> = new Set([
	'team.create',
	'team.rename',
	'team.move',
	'team.set_lead',
	'team.delete',
	'team.set_title',
	'member.add_person',
	'member.update_person',
	'member.remove',
	'member.set_reports_to',
	'member.move',
	'project.add_team',
	'project.remove_team'
]);

/** Closed table mapping implemented B2 organization commands to `/api/v1/organizations/{org}/{route}`. */
function mapOrgMutation(
	command: OrgCommand
): { route: string; params: Record<string, unknown> } | undefined {
	switch (command.method) {
		case 'team.create': {
			const { team_id, name, owner_team_id } = command.params;
			return {
				route: 'teams/create',
				params: { team_id, name, ...(owner_team_id !== undefined ? { owner_team_id } : {}) }
			};
		}
		case 'team.rename': {
			const { team_id, expected_version, name } = command.params;
			return { route: 'teams/rename', params: { team_id, expected_version, name } };
		}
		case 'team.move': {
			const { team_id, expected_version, owner_team_id } = command.params;
			return {
				route: 'teams/move',
				params: {
					team_id,
					expected_version,
					...(owner_team_id !== undefined ? { owner_team_id } : {})
				}
			};
		}
		case 'team.set_lead': {
			const { team_id, expected_version, lead_member_id } = command.params;
			return {
				route: 'teams/set-lead',
				params: {
					team_id,
					expected_version,
					...(lead_member_id !== undefined ? { lead_member_id } : {})
				}
			};
		}
		case 'team.delete': {
			const { team_id, expected_version } = command.params;
			return { route: 'teams/delete', params: { team_id, expected_version } };
		}
		case 'team.set_title': {
			const { membership_id, expected_version, title_in_team } = command.params;
			return {
				route: 'teams/set-title',
				params: {
					membership_id,
					expected_version,
					...(title_in_team !== undefined ? { title_in_team } : {})
				}
			};
		}
		case 'member.add_person': {
			const { member_id, name, contact, handle, home_team_id } = command.params;
			return {
				route: 'members/add-person',
				params: {
					member_id,
					name,
					...(contact !== undefined ? { contact } : {}),
					...(handle !== undefined ? { handle } : {}),
					...(home_team_id !== undefined ? { home_team_id } : {})
				}
			};
		}
		case 'member.update_person': {
			const { member_id, expected_version, name, contact, clear_contact } = command.params;
			return {
				route: 'members/update-person',
				params: {
					member_id,
					expected_version,
					...(name !== undefined ? { name } : {}),
					...(contact !== undefined ? { contact } : {}),
					...(clear_contact ? { clear_contact } : {})
				}
			};
		}
		case 'member.remove': {
			const { member_id, expected_version } = command.params;
			return { route: 'members/remove', params: { member_id, expected_version } };
		}
		case 'member.set_reports_to': {
			const { member_id, expected_version, reports_to_member_id } = command.params;
			return {
				route: 'members/set-reports-to',
				params: {
					member_id,
					expected_version,
					...(reports_to_member_id !== undefined ? { reports_to_member_id } : {})
				}
			};
		}
		case 'member.move': {
			const { membership_id, expected_version, new_membership_id, to_team_id, title_in_team } =
				command.params;
			return {
				route: 'members/move',
				params: {
					membership_id,
					expected_version,
					new_membership_id,
					to_team_id,
					...(title_in_team !== undefined ? { title_in_team } : {})
				}
			};
		}
		case 'project.add_team': {
			const { project_team_id, project_id, team_id } = command.params;
			return {
				route: 'projects/add-team',
				params: { project_team_id, project_id, team_id }
			};
		}
		case 'project.remove_team': {
			const { project_team_id, expected_version, areas_to_team_id } = command.params;
			return {
				route: 'projects/remove-team',
				params: {
					project_team_id,
					expected_version,
					...(areas_to_team_id !== undefined ? { areas_to_team_id } : {})
				}
			};
		}
		default:
			return undefined;
	}
}

function commandFailureFallback(status: number): ApiErrorCode {
	if (status === 401 || status === 403) return 'not_authorized';
	if (status === 404) return 'not_found';
	if (status === 409) return 'conflict';
	if (status === 410) return 'expired';
	return 'unavailable';
}

const defaultNewId = () =>
	formatUuidV7(Date.now(), globalThis.crypto.getRandomValues(new Uint8Array(10)));

/**
 * Production HTTP adapter for the organization gateway routes (`/api/v1/organizations/{org}/...`).
 * Never falls back to a browser reducer or sample data.
 */
export function createHttpOrgApi({
	http,
	instanceId,
	newId = defaultNewId
}: {
	http: OrgHttpTransport;
	instanceId: () => string | undefined;
	newId?: () => string;
}): OrgApi {
	return Object.freeze({
		async view(organizationId: string) {
			let reply: Reply;
			try {
				reply = await http.read(
					`/api/v1/organizations/${encodeURIComponent(organizationId)}/snapshot`
				);
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'The organization could not be reached.'
				} satisfies AppError;
			}
			if (reply.status !== 200) {
				const fallback: ApiErrorCode =
					reply.status === 401 || reply.status === 403
						? 'not_authorized'
						: reply.status === 404
							? 'not_found'
							: 'unavailable';
				throw readAppError(reply.body, fallback, 'The organization could not be loaded.');
			}
			const root = reply.body as { result?: { snapshot?: unknown } } | null;
			const snapshot = root?.result?.snapshot;
			if (!isSnapshot(snapshot)) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'The organization snapshot could not be read.'
				} satisfies AppError;
			}
			return { snapshot: normalizeKinds(snapshot), sample: false };
		},

		async command(
			organizationId: string,
			input: OrgCommandInput | OrgCommand
		): Promise<CommandAccepted> {
			if (!SUPPORTED_METHODS.has(input.method)) {
				throw {
					source: 'api',
					code: 'unsupported',
					message: 'This action is not available over the browser gateway yet.'
				} satisfies AppError;
			}
			const command = prepareOrgCommand(input, newId);
			const mapped = mapOrgMutation(command);
			if (!mapped) {
				throw {
					source: 'api',
					code: 'unsupported',
					message: 'This action is not available over the browser gateway yet.'
				} satisfies AppError;
			}
			const instance = instanceId();
			if (!instance) {
				throw {
					source: 'api',
					code: 'not_authorized',
					message: 'Sign in to continue.'
				} satisfies AppError;
			}
			const body = {
				operation_id: command.params.command_id,
				scope: { kind: 'organization', instance_id: instance, org_id: organizationId },
				params: mapped.params
			};
			let reply: Reply;
			try {
				reply = await http.submit(
					`/api/v1/organizations/${encodeURIComponent(organizationId)}/${mapped.route}`,
					body
				);
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'The change could not be confirmed.'
				} satisfies AppError;
			}
			const root =
				reply.body && typeof reply.body === 'object'
					? (reply.body as Record<string, unknown>)
					: undefined;
			if (reply.status === 200 && root?.outcome === 'completed') {
				return { status: 'committed' };
			}
			if (reply.status === 504 || root?.outcome === 'unknown') {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'The outcome of this request is not confirmed yet.'
				} satisfies AppError;
			}
			throw readAppError(
				reply.body,
				commandFailureFallback(reply.status),
				'The change was refused.'
			);
		},

		onFeed: unavailableCall,
		retryFeed: unavailableCall,
		feedLive: unavailableCall
	});
}
