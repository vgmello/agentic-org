import type { ContentRef } from './wire';

const record = (value: unknown): Record<string, unknown> | undefined =>
	value !== null && typeof value === 'object' && !Array.isArray(value)
		? (value as Record<string, unknown>)
		: undefined;
const text = (value: unknown): string | undefined =>
	typeof value === 'string' && value.length > 0 ? value : undefined;
function pair(value: unknown): string | undefined {
	const fields = record(value);
	if (!fields) return;
	const label = (key: string) => (fields[key] === null ? 'None' : text(fields[key]));
	const team = label('assigned_team_id'),
		employee = label('assignee_id');
	return team && employee ? `team ${team}, assignee ${employee}` : undefined;
}
/** Display only. Content integrity/scope is checked by the chunk adapter/server.
 * Revision comes from its decimal-string reference, never JSON.parse's number.
 * No parsed extension or number is reserialized: the caller retains original text.
 */
export function eventSummary(source: string, reference: ContentRef): string {
	if (reference.kind !== 'event') return 'Verified public task Markdown.';
	let event: Record<string, unknown> | undefined;
	try {
		event = record(JSON.parse(source));
	} catch {
		/* Keep original source available. */
	}
	const change = record(event?.change),
		tag = text(change?.type);
	const actorKind = text(event?.actor_kind) ?? 'unknown';
	const actor = text(event?.actor_id);
	const by = `Actor: ${actorKind}${actor ? ` ${actor}` : ''}.`;
	const prefix = `Revision ${reference.revision}.`;
	if (tag === 'assignment_changed') {
		const before = pair(change?.from),
			after = pair(change?.to);
		if (before && after) return `${prefix} Assignment changed from ${before} to ${after}. ${by}`;
	}
	if (tag === 'created') return `${prefix} Task created; full public snapshot retained. ${by}`;
	if (tag === 'history_baseline')
		return `${prefix} Imported snapshot baseline. Earlier history is unavailable. ${by}`;
	return `${prefix} Unknown activity${tag ? ` (${tag})` : ''}. Original event retained. ${by}`;
}
