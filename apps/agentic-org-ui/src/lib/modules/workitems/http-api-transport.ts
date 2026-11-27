import type { AppError } from '$lib/proto';
import {
	gatewayStatusCode,
	readGatewayAppError,
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { inferTaskField } from './http-api-payloads';

export function createWorkItemTransport(http: GatewayHttpTransport) {
	async function readJson(
		path: string,
		fallbackMessage: string,
		signal?: AbortSignal
	): Promise<Record<string, unknown>> {
		let reply: GatewayReply;
		try {
			reply = await (signal ? http.read(path, signal) : http.read(path));
		} catch (err) {
			if (signal?.aborted) throw err;
			throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies AppError;
		}
		return unwrapGatewayResult(reply, fallbackMessage);
	}

	async function postJson(
		path: string,
		body: Record<string, unknown>,
		fallbackMessage: string
	): Promise<Record<string, unknown>> {
		let reply: GatewayReply;
		try {
			reply = await http.submit(path, body);
		} catch {
			throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies AppError;
		}
		if (reply.status !== 200) {
			const err = readGatewayAppError(reply.body, gatewayStatusCode(reply.status), fallbackMessage);
			const field = inferTaskField(err);
			throw field ? { ...err, field } : err;
		}
		return unwrapGatewayResult(reply, fallbackMessage);
	}

	return { readJson, postJson };
}
