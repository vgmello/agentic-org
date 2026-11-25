import { API_ERROR_CODES, type ApiErrorCode, type AppError } from '$lib/proto';
import { readJsonObject, readResponse } from './wire';

export type GatewayReply = { status: number; body: unknown };

export interface GatewayHttpTransport {
	read(path: string, signal?: AbortSignal): Promise<GatewayReply>;
	submit(path: string, body: unknown, signal?: AbortSignal): Promise<GatewayReply>;
}

const KNOWN_CODES: ReadonlySet<string> = new Set(API_ERROR_CODES);

export function asApiErrorCode(code: unknown): ApiErrorCode {
	return typeof code === 'string' && KNOWN_CODES.has(code) ? (code as ApiErrorCode) : 'unknown';
}

export function gatewayStatusCode(status: number): ApiErrorCode {
	if (status === 401 || status === 403) return 'not_authorized';
	if (status === 404) return 'not_found';
	if (status === 409) return 'conflict';
	if (status === 410) return 'expired';
	if (status === 400 || status === 422) return 'invalid_request';
	return 'unavailable';
}

export function readGatewayAppError(
	body: unknown,
	fallbackCode: ApiErrorCode,
	fallbackMessage: string
): AppError {
	const root = body && typeof body === 'object' ? (body as Record<string, unknown>) : undefined;
	const err =
		root?.error && typeof root.error === 'object'
			? (root.error as Record<string, unknown>)
			: undefined;
	return {
		source: 'api',
		code: err ? asApiErrorCode(err.code) : fallbackCode,
		message: typeof err?.message === 'string' && err.message ? err.message : fallbackMessage
	};
}

export function unwrapGatewayResult(
	reply: GatewayReply,
	fallbackMessage: string
): Record<string, unknown> {
	if (reply.status !== 200) {
		throw readGatewayAppError(reply.body, gatewayStatusCode(reply.status), fallbackMessage);
	}
	let envelope: ReturnType<typeof readResponse<Record<string, unknown>>>;
	try {
		envelope = readResponse(reply.body, readJsonObject);
	} catch {
		throw {
			source: 'api',
			code: 'unavailable',
			message: fallbackMessage
		} satisfies AppError;
	}
	if ('error' in envelope) {
		throw {
			source: 'api',
			code: 'unavailable',
			message: fallbackMessage
		} satisfies AppError;
	}
	return envelope.result;
}
