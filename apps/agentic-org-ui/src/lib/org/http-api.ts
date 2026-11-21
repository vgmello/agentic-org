import {
	API_ERROR_CODES,
	type ApiErrorCode,
	type AppError,
	type CommandAccepted,
	type OrgSnapshot
} from '$lib/proto';
import { unavailableCall } from '$lib/platform/unavailable-api';
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
		Array.isArray(s.members) &&
		Array.isArray(s.memberships)
	);
}

/**
 * Production HTTP adapter for the organization gateway routes (`/api/v1/organizations/{org}/...`).
 * Never falls back to a browser reducer or sample data.
 */
export function createHttpOrgApi({
	http,
	instanceId
}: {
	http: OrgHttpTransport;
	instanceId: () => string | undefined;
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
					reply.status === 401
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
			const command = ready(input);
			if (command.method !== 'team.create') {
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
			const { command_id, team_id, name, owner_team_id } = command.params;
			const body = {
				operation_id: command_id,
				scope: { kind: 'organization', instance_id: instance, org_id: organizationId },
				params: {
					team_id,
					name,
					...(owner_team_id !== undefined ? { owner_team_id } : {})
				}
			};
			let reply: Reply;
			try {
				reply = await http.submit(
					`/api/v1/organizations/${encodeURIComponent(organizationId)}/teams`,
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
			const fallback: ApiErrorCode =
				reply.status === 401
					? 'not_authorized'
					: reply.status === 404
						? 'not_found'
						: reply.status === 409
							? 'conflict'
							: 'unavailable';
			throw readAppError(reply.body, fallback, 'The change was refused.');
		},

		onFeed: unavailableCall,
		retryFeed: unavailableCall,
		feedLive: unavailableCall
	});
}
