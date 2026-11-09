// The rules around the effective set that the screens state in words: the one open branch rule (spec §4), who approves a
// change at each level (decision 8, PROC-11), and the refusal of a second branch. Pure.

import type { EffectiveSet, Level, ProcessDoc, TeamRef } from './types';

/** The organization's first process: it carries the one open branch rule. */
export const BRANCH_KEY = 'branch-and-merge';

export interface RuleState {
	on: boolean;
	/** In the spec's words: "On", "On · turning it off needs approval by someone other than its author (<author>)", or "Turned off for <project>: <reason>". */
	text: string;
	/** When a turning-off merged but does not count: who wrote it. */
	selfApprovedBy?: string;
}

/**
 * The rule is on unless the computed set of the task's project has a turning-off of branch-and-merge that someone other than its
 * author approved. A missing, clashed or invalid process leaves it on, and so does a set that could not be computed (`set` undefined).
 */
export function branchRule(set: EffectiveSet | undefined, projectName: string): RuleState {
	const off = set?.turnedOff.find((t) => t.key === BRANCH_KEY);
	if (!off) return { on: true, text: 'On' };
	const { author, approvedBy } = off.doc;
	// An approval counts only when the author is known and someone else approved: the rule fails closed.
	if (approvedBy && author && approvedBy !== author) return { on: false, text: `Turned off for ${off.scopeName ?? projectName}: ${off.reason}` };
	return { on: true, text: `On · turning it off needs approval by someone other than its author (${author ?? 'unknown'})`, ...(author ? { selfApprovedBy: author } : {}) };
}

export interface Refusal {
	code: 'one_branch_open';
	message: string;
}

/** "Your branch task/chk-T-17-… isn't merged yet (waiting for review). Finish it before starting chk-T-18." (until stages are reported: no stage). */
export function oneBranchRefusal(branch: string, stage: string | undefined, target: string): Refusal {
	return { code: 'one_branch_open', message: `Your branch ${branch} isn't merged yet${stage ? ` (${stage})` : ''}. Finish it before starting ${target}.` };
}

/** What a lead sees in Needs you: "River can't start chk-T-18: task/chk-T-17-… isn't merged (waiting for review)." */
export function leadRefusalLine(agent: string, target: string, branch: string, stage: string | undefined): string {
	return `${agent} can't start ${target}: ${branch} isn't merged${stage ? ` (${stage})` : ' yet'}.`;
}

export interface ApprovalContext {
	ownerId: string;
	teams: TeamRef[];
	/** The leads of the project's teams (a shared project has one per team). */
	projectLeads: string[];
}

export const NOT_BY_AUTHOR = 'Another approver or the owner must approve this.';

/** Who may approve a change to a process at this level: the owner always; the team's lead; any lead of the project's teams. */
export function approversOf(level: Level, scope: string | undefined, ctx: ApprovalContext): string[] {
	const owner = [ctx.ownerId];
	if (level === 'team') {
		const lead = ctx.teams.find((t) => t.id === scope)?.lead_member_id;
		return lead ? [lead, ...owner] : owner;
	}
	if (level === 'project') return [...ctx.projectLeads, ...owner];
	return owner; // the organization (the owner; a lead the owner names is proposed, U27) and private processes
}

/**
 * Why `person` can't approve a change to `doc` (written by `author`), or nothing when they can. No one approves their own request,
 * except the owner, who has no one above.
 */
export function cannotApprove(doc: Pick<ProcessDoc, 'level' | 'scope'>, author: string, person: string, ctx: ApprovalContext): string | undefined {
	if (!approversOf(doc.level, doc.scope, ctx).includes(person)) return 'Only this level’s approvers can approve this.';
	if (person === author && person !== ctx.ownerId) return NOT_BY_AUTHOR;
	return undefined;
}
