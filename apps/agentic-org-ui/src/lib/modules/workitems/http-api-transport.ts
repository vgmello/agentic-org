import type { AppError } from '$lib/proto';
import type { OrgApi } from '$lib/modules/organization/api';
import {
	gatewayStatusCode,
	readGatewayAppError,
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { inferTaskField, isWireProject, isWireTaskView } from './http-api-payloads';
import {
	buildOrgLookup,
	type OrgLookup,
	type WireProject,
	type WireTaskInspect,
	type WireTaskView
} from './wire-map';

export function orgBase(orgId: string): string {
	return `/api/v1/organizations/${encodeURIComponent(orgId)}`;
}

export function createWorkItemTransport(http: GatewayHttpTransport, orgApi: Pick<OrgApi, 'view'>) {
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

	async function readProjects(
		organizationId: string,
		signal?: AbortSignal
	): Promise<WireProject[]> {
		try {
			const res = await readJson(
				`${orgBase(organizationId)}/projects?include_archived=true`,
				'Projects could not be loaded.',
				signal
			);
			return Array.isArray(res.projects) ? res.projects.filter(isWireProject) : [];
		} catch (err) {
			if (signal?.aborted) throw err;
			return [];
		}
	}

	async function loadLookup(
		organizationId: string,
		signal?: AbortSignal
	): Promise<{ lookup: OrgLookup; projects: WireProject[] }> {
		const [view, projects] = await Promise.all([
			orgApi.view(organizationId, signal),
			readProjects(organizationId, signal)
		]);
		return { lookup: buildOrgLookup(view.snapshot, projects), projects };
	}

	async function readTasks(
		organizationId: string,
		params?: URLSearchParams,
		signal?: AbortSignal
	): Promise<WireTaskView[]> {
		const qs = params?.toString();
		const res = await readJson(
			`${orgBase(organizationId)}/tasks${qs ? `?${qs}` : ''}`,
			'Work items could not be loaded.',
			signal
		);
		if (!Array.isArray(res.tasks)) {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'The tasks response could not be read.'
			} satisfies AppError;
		}
		return res.tasks.filter(isWireTaskView);
	}

	async function inspectRaw(organizationId: string, refOrId: string): Promise<WireTaskInspect> {
		const res = await readJson(
			`${orgBase(organizationId)}/tasks/${encodeURIComponent(refOrId)}`,
			'The work item could not be loaded.'
		);
		if (!isWireTaskView(res.task)) {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'The work item could not be read.'
			} satisfies AppError;
		}
		return res as unknown as WireTaskInspect;
	}

	return { readJson, postJson, readProjects, loadLookup, readTasks, inspectRaw };
}
