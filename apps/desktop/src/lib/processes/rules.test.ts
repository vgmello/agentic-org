import { describe, expect, it } from 'vitest';
import { resolve } from './resolve';
import { approversOf, BRANCH_KEY, branchRule, cannotApprove, leadRefusalLine, NOT_BY_AUTHOR, oneBranchRefusal } from './rules';
import { SAMPLE_PROJECTS, SAMPLE_TEAMS, sampleProcessDocs } from './sample';

const docs = sampleProcessDocs();
const setFor = (key: string) => resolve({ docs, teams: SAMPLE_TEAMS, project: SAMPLE_PROJECTS.find((p) => p.key === key)! });

describe('PROC-14 the one open branch rule', () => {
	it('is on for a project that did not turn it off', () => {
		expect(branchRule(setFor('atl'), 'Atlas')).toEqual({ on: true, text: 'On' });
	});

	it('is off for a project whose turning-off someone other than its author approved', () => {
		const set = resolve({
			docs: docs.map((d) => (d.key === BRANCH_KEY && d.location.scope === 'onb' ? { ...d, approvedBy: 'you' } : d)),
			teams: SAMPLE_TEAMS,
			project: SAMPLE_PROJECTS.find((p) => p.key === 'onb')!
		});
		expect(branchRule(set, 'Onboarding kit')).toEqual({ on: false, text: 'Turned off for Onboarding kit: a single agent works here' });
	});

	it('stays on, and says why, when the turning-off was approved by its own author', () => {
		const set = setFor('onb');
		expect(set.entries.map((e) => e.key)).toContain('branch-and-merge'); // the process is still in force
		const rule = branchRule(set, 'Onboarding kit');
		expect(rule.on).toBe(true);
		expect(rule.text).toBe('On · turning it off needs approval by someone other than its author (hana)');
		expect(rule.selfApprovedBy).toBe('hana');
	});

	it('stays on when the process is missing, clashing or invalid, and when the set could not be computed', () => {
		expect(branchRule(undefined, 'Atlas').on).toBe(true);
		const without = resolve({ docs: docs.filter((d) => d.key !== BRANCH_KEY), teams: SAMPLE_TEAMS, project: SAMPLE_PROJECTS[0] });
		expect(branchRule(without, 'Atlas').on).toBe(true);
		const broken = resolve({ docs: docs.map((d) => (d.key === BRANCH_KEY && d.location.scope === 'onb' ? { ...d, problems: [{ field: 'status', message: 'unknown' }] } : d)), teams: SAMPLE_TEAMS, project: SAMPLE_PROJECTS.find((p) => p.key === 'onb')! });
		expect(branchRule(broken, 'Onboarding kit').on).toBe(true);
		const clash = resolve({ docs: [...docs, { ...docs.find((d) => d.key === BRANCH_KEY && d.location.scope === 'onb')!, id: 'doc-twin' }], teams: SAMPLE_TEAMS, project: SAMPLE_PROJECTS.find((p) => p.key === 'onb')! });
		expect(branchRule(clash, 'Onboarding kit').on).toBe(true);
	});

	it('refuses starting new work with the branch and its stage, in the spec’s words', () => {
		expect(oneBranchRefusal('task/chk-T-17-checkout-form', 'waiting for review', 'chk-T-18')).toEqual({
			code: 'one_branch_open',
			message: "Your branch task/chk-T-17-checkout-form isn't merged yet (waiting for review). Finish it before starting chk-T-18."
		});
		expect(oneBranchRefusal('task/chk-T-17-x', undefined, 'chk-T-18').message).toBe("Your branch task/chk-T-17-x isn't merged yet. Finish it before starting chk-T-18.");
		expect(leadRefusalLine('River', 'chk-T-18', 'task/chk-T-17-x', 'waiting for review')).toBe("River can't start chk-T-18: task/chk-T-17-x isn't merged (waiting for review).");
		expect(leadRefusalLine('River', 'chk-T-18', 'task/chk-T-17-x', undefined)).toBe("River can't start chk-T-18: task/chk-T-17-x isn't merged yet.");
	});
});

describe('PROC-11 who approves a change', () => {
	const ctx = { ownerId: 'you', teams: SAMPLE_TEAMS, projectLeads: ['noor', 'juno'] };

	it('names the approvers of each level: the owner; the team’s lead and the owner; any lead of the project’s teams and the owner', () => {
		expect(approversOf('organization', undefined, ctx)).toEqual(['you']);
		expect(approversOf('team', 'web', ctx)).toEqual(['noor', 'you']);
		expect(approversOf('project', 'bea', ctx)).toEqual(['noor', 'juno', 'you']);
		expect(approversOf('project', 'scr', { ...ctx, projectLeads: [] })).toEqual(['you']);
		expect(approversOf('agent', 'noor', ctx)).toEqual(['you']);
	});

	it('no one approves their own change, except the owner', () => {
		expect(cannotApprove({ level: 'team', scope: 'web' }, 'noor', 'noor', ctx)).toBe(NOT_BY_AUTHOR);
		expect(cannotApprove({ level: 'team', scope: 'web' }, 'noor', 'you', ctx)).toBeUndefined();
		expect(cannotApprove({ level: 'organization' }, 'you', 'you', ctx)).toBeUndefined();
		expect(cannotApprove({ level: 'project', scope: 'bea' }, 'noor', 'juno', ctx)).toBeUndefined();
	});

	it('someone who is not an approver of the level cannot approve at all', () => {
		expect(cannotApprove({ level: 'team', scope: 'web' }, 'kai', 'juno', ctx)).toMatch(/Only this level/);
	});
});
