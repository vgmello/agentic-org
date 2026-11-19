import type { ActivityQuery, ActivityRow } from './types';

/**
 * Whether a row belongs in a feed. Decisions are for auditing: only the Decisions filter shows them.
 * Status changes are noisy: hidden unless asked for. Search matches the actor, the text and the preview.
 */
export function matchesQuery(row: ActivityRow, query: ActivityQuery): boolean {
	if (row.kind === 'decision' && query.kind !== 'decision') return false;
	if (row.status && !query.includeStatus) return false;
	if (query.kind !== 'all' && row.kind !== query.kind) return false;
	if (query.team && !row.teamIds.includes(query.team)) return false;
	if (query.project && !row.projectIds.includes(query.project)) return false;
	if (query.member && row.actor.id !== query.member && !row.memberIds.includes(query.member))
		return false;
	const needle = query.q?.trim().toLowerCase();
	if (needle) {
		const haystack = `${row.actor.name} ${row.text} ${row.preview ?? ''}`.toLowerCase();
		if (!needle.split(/\s+/).every((word) => haystack.includes(word))) return false;
	}
	return true;
}

/** The query's non-default parts as URL parameters, and back. */
export function queryToParams(query: ActivityQuery): URLSearchParams {
	const params = new URLSearchParams();
	if (query.team) params.set('team', query.team);
	if (query.member) params.set('member', query.member);
	if (query.project) params.set('project', query.project);
	if (query.kind !== 'all') params.set('kind', query.kind);
	if (query.q?.trim()) params.set('q', query.q.trim());
	if (query.includeStatus) params.set('status', '1');
	return params;
}

const KINDS = new Set(['all', 'work', 'conversation', 'decision', 'run', 'org']);
export function queryFromParams(params: URLSearchParams): ActivityQuery {
	const kind = params.get('kind') ?? 'all';
	return {
		team: params.get('team') || undefined,
		member: params.get('member') || undefined,
		project: params.get('project') || undefined,
		kind: (KINDS.has(kind) ? kind : 'all') as ActivityQuery['kind'],
		q: params.get('q') ?? undefined,
		includeStatus: params.get('status') === '1'
	};
}
