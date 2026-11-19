// Health for Agent setup (our-org-in-the-app §5): Ready, Needs setup: {reason}, Paused, or Runtime readiness
// unknown, decided only from configuration and the office's readiness facts. Needs setup wins over Paused; an
// unknown readiness is said so, never read as ready. Health is not presence and grants nothing.
import type { AgentSetupRow, Blocker } from './types';

export interface HealthReason {
	/** Stable, for tests and keys: a document 11 code, or this module's own for configuration facts. */
	code: string;
	text: string;
	fix?: string;
}

export type Health =
	| { kind: 'needs_setup'; reasons: HealthReason[] }
	| { kind: 'paused' }
	| { kind: 'unknown'; why: string }
	| { kind: 'ready' };

export type HealthFacts = Pick<
	AgentSetupRow,
	'name' | 'status' | 'homeTeam' | 'execution' | 'placement' | 'readiness' | 'catalog'
>;

/** The office's own word for a model it doesn't offer: the strict predicate below must not repeat it. */
const MODEL_NOT_OFFERED = 'MODEL_NOT_ENTITLED';

/**
 * The strict missing-model predicate (our-org-in-the-app §6 Problems): an exact selected model known absent from
 * the runtime's reported list. An unset model, an unreadable list, no runtime or no office doesn't establish it.
 */
export function modelMissing(facts: HealthFacts): boolean {
	const e = facts.execution;
	if (!e || !e.modelId || !facts.placement || e.runtimes.length === 0) return false;
	if (!facts.catalog.reported) return false;
	return !facts.catalog.models.includes(e.modelId);
}

/** Every reason the agent needs setup, in the order a person would fix them. Empty when there is none. */
export function setupReasons(facts: HealthFacts): HealthReason[] {
	const reasons: HealthReason[] = [];
	if (!facts.homeTeam)
		reasons.push({
			code: 'HOME_TEAM_MISSING',
			text: 'Home team missing: this record is invalid',
			fix: `Move ${facts.name} to a home team from its profile.`
		});
	const e = facts.execution;
	if (!e)
		reasons.push({
			code: 'EXECUTION_NOT_CONFIGURED',
			text: 'Execution not configured',
			fix: 'Choose a model profile and a model with Edit.'
		});
	else if (!e.enabled)
		reasons.push({
			code: 'EXECUTION_DISABLED',
			text: 'Execution is turned off in its configuration',
			fix: 'Saving a model profile and model with Edit turns it back on.'
		});
	if (!facts.placement)
		reasons.push({
			code: 'UNPLACED',
			text: 'Unplaced',
			fix: 'Choose an office for it in Offices, under Place or offboard.'
		});
	const blockers: Blocker[] = facts.readiness.known ? facts.readiness.blockers : [];
	for (const b of blockers)
		reasons.push({ code: b.code, text: b.reason, ...(b.fix ? { fix: b.fix } : {}) });
	if (modelMissing(facts) && !blockers.some((b) => b.code === MODEL_NOT_OFFERED) && e)
		reasons.push({
			code: MODEL_NOT_OFFERED,
			text: `${e.modelId} isn't offered by ${e.runtimes[0]} on ${facts.placement!.office.name}`,
			fix: 'Choose a model the runtime offers with Edit, or wait until the runtime offers it.'
		});
	return reasons;
}

export function health(facts: HealthFacts): Health {
	const reasons = setupReasons(facts);
	if (reasons.length > 0) return { kind: 'needs_setup', reasons };
	if (facts.status === 'paused') return { kind: 'paused' };
	if (!facts.readiness.known) return { kind: 'unknown', why: facts.readiness.why };
	return { kind: 'ready' };
}

/** The Health word as the spec writes it. */
export const HEALTH_WORD: Record<Health['kind'], string> = {
	ready: 'Ready',
	needs_setup: 'Needs setup',
	paused: 'Paused',
	unknown: 'Runtime readiness unknown'
};

/** The whole Health phrase for a row and for assistive technology: "Needs setup: Unplaced (and 1 more)". */
export function healthText(h: Health): string {
	if (h.kind !== 'needs_setup') return HEALTH_WORD[h.kind];
	const more = h.reasons.length - 1;
	return `Needs setup: ${h.reasons[0].text}${more > 0 ? ` (and ${more} more)` : ''}`;
}

/** How many agents need setup, for the list's description. */
export const needsSetupCount = (rows: HealthFacts[]) =>
	rows.filter((r) => health(r).kind === 'needs_setup').length;
