import { asNum, asRec, asStr, asStrList } from '$lib/platform/http/decode';
import type { Member } from '$lib/proto';
import type { Blocker, ConnectionState, Placement, ProfileModel, Readiness } from './types';

export interface WireConnection {
	id: string;
	name: string;
	runtimeDriver: string;
	enabled: boolean;
}

export interface WireProfile {
	id: string;
	name: string;
	connectionIds: string[];
	archived: boolean;
}

export interface WireExecution {
	modelProfileId: string;
	modelId: string;
	effort?: string;
	enabled: boolean;
	revision: number;
	toolsConfig: string[];
	permissionPreset?: string;
	systemPromptMarkdown: string;
}

export interface WireReadiness {
	ready: boolean;
	status: string;
	candidates: {
		connectionId: string;
		eligible: boolean;
		blockerCode?: string;
		reason?: string;
		fix?: string;
	}[];
}

export function runtimeLabel(driver: string | undefined): string {
	switch ((driver ?? '').toLowerCase()) {
		case 'claude':
		case 'claude_code':
		case 'anthropic':
			return 'Claude Code';
		case 'codex':
		case 'openai':
			return 'Codex';
		case 'agy':
		case 'antigravity':
		case 'google':
			return 'Antigravity';
		default:
			return driver ? driver : 'Runtime';
	}
}

export function runtimeFromMember(member: Member): string[] {
	const r = member.agent?.runtime;
	if (!r) return [];
	if (typeof r === 'string') return r === 'unknown' ? [] : [runtimeLabel(r)];
	return r.other ? [r.other] : [];
}

export function decodeConnections(raw: unknown): WireConnection[] {
	const rec = asRec(raw);
	const list = Array.isArray(rec?.connections) ? rec.connections : [];
	return list
		.map((item): WireConnection | undefined => {
			const r = asRec(item);
			const id = asStr(r?.id);
			if (!r || !id) return undefined;
			return {
				id,
				name: asStr(r.name) ?? asStr(r.key) ?? id,
				runtimeDriver: asStr(r.runtime_driver) ?? asStr(r.provider_type) ?? '',
				enabled: r.enabled !== false && r.archived !== true
			};
		})
		.filter((c): c is WireConnection => c !== undefined);
}

export function decodeProfiles(raw: unknown): WireProfile[] {
	const rec = asRec(raw);
	const list = Array.isArray(rec?.profiles) ? rec.profiles : [];
	return list
		.map((item): WireProfile | undefined => {
			const r = asRec(item);
			const id = asStr(r?.id);
			if (!r || !id) return undefined;
			return {
				id,
				name: asStr(r.name) ?? asStr(r.key) ?? id,
				connectionIds: asStrList(r.connection_ids),
				archived: r.archived === true
			};
		})
		.filter((p): p is WireProfile => p !== undefined && !p.archived);
}

export function decodeExecution(raw: unknown): WireExecution | null {
	const r = asRec(raw);
	const modelProfileId = asStr(r?.model_profile_id);
	const modelId = asStr(r?.model_id);
	if (!r || !modelProfileId || !modelId) return null;
	return {
		modelProfileId,
		modelId,
		effort: asStr(r.effort),
		enabled: r.enabled !== false,
		revision: asNum(r.revision) ?? 1,
		toolsConfig: asStrList(r.tools_config),
		permissionPreset: asStr(r.permission_preset),
		systemPromptMarkdown: asStr(r.system_prompt_markdown) ?? ''
	};
}

export function decodeReadiness(raw: unknown): WireReadiness | undefined {
	const r = asRec(raw);
	if (!r) return undefined;
	const rawCands = Array.isArray(r.candidates) ? r.candidates : [];
	const candidates = rawCands
		.map((item) => {
			const c = asRec(item);
			const connectionId = asStr(c?.connection_id);
			if (!c || !connectionId) return undefined;
			return {
				connectionId,
				eligible: c.eligible === true,
				blockerCode: asStr(c.blocker_code) ?? asStr(c.code),
				reason: asStr(c.reason) ?? asStr(c.message),
				fix: asStr(c.fix)
			};
		})
		.filter((c): c is NonNullable<typeof c> => c !== undefined);
	return {
		ready: r.ready === true,
		status: asStr(r.status) ?? 'UNKNOWN',
		candidates
	};
}

export function decodeProfileModels(raw: unknown): { models: ProfileModel[]; asOf?: string } {
	const r = asRec(raw);
	const list = Array.isArray(r?.models) ? r.models : [];
	let asOf: string | undefined;
	const models: ProfileModel[] = [];
	for (const item of list) {
		const m = asRec(item);
		const id = asStr(m?.model_id) ?? asStr(m?.id);
		if (!m || !id) continue;
		const itemAsOf = asStr(m.as_of);
		if (itemAsOf && (!asOf || itemAsOf > asOf)) asOf = itemAsOf;
		const supporting = Array.isArray(m.supporting_connections)
			? m.supporting_connections.length
			: (asNum(m.supporting) ?? 0);
		const ready = asNum(m.ready_connections) ?? asNum(m.ready) ?? 0;
		models.push({ id, supporting, ready, stale: m.stale === true });
	}
	return { models, asOf };
}

export function mapConnectionState(
	conn: WireConnection,
	cand: WireReadiness['candidates'][number] | undefined
): ConnectionState {
	if (!conn.enabled) return 'unknown';
	if (cand?.eligible) return 'ready';
	const code = (cand?.blockerCode ?? '').toUpperCase();
	if (code.includes('AUTH') || code.includes('SIGN')) return 'signed_out';
	if (code.includes('DRIVER') || code.includes('INSTALL')) return 'not_installed';
	if (code.includes('BINDING')) return 'binding_missing';
	if (code.includes('COOLDOWN') || code.includes('QUOTA')) return 'cooling_down';
	return 'unknown';
}

export function mapReadiness(
	wire: WireReadiness | undefined,
	placement: Placement | null,
	checkedAt: string
): Readiness {
	if (!placement) return { known: false, why: 'This agent is not placed in an office yet.' };
	if (!wire) return { known: false, why: 'Readiness was not reported by the office.' };
	if (wire.ready) return { known: true, checkedAt, blockers: [] };
	if (wire.status === 'NO_EXECUTION_CONFIG') {
		return {
			known: true,
			checkedAt,
			blockers: [
				{
					code: 'NO_EXECUTION_CONFIG',
					reason: 'Execution is not configured for this agent yet.',
					fix: 'Choose a model profile and model below.'
				}
			]
		};
	}
	const blockers: Blocker[] = wire.candidates
		.filter((c) => !c.eligible)
		.map((c) => ({
			code: c.blockerCode ?? wire.status,
			reason: c.reason ?? `Connection ${c.connectionId} is not eligible (${wire.status}).`,
			...(c.fix ? { fix: c.fix } : {})
		}));
	if (blockers.length === 0) {
		blockers.push({
			code: wire.status,
			reason: `Office reported status: ${wire.status}.`
		});
	}
	return { known: true, checkedAt, blockers };
}
