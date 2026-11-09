import type { ApprovalContext, ProcessDoc, ProjectRef, TeamRef } from './types';

// Invented processes for the sample organization (src/lib/org/sample.ts): Engineering with Web and Platform under it, Design,
// People; the project Atlas (Platform), Beacon (Web and Design, shared), Onboarding (People). Shown as Sample data.

let seq = 0;
const doc = (over: Partial<ProcessDoc> & Pick<ProcessDoc, 'key' | 'name' | 'level'> & { scope?: string }): ProcessDoc => {
	seq += 1;
	const { scope, level, ...rest } = over;
	return {
		id: `doc-01JC0PROC${String(seq).padStart(15, '0')}`,
		version: 1,
		owner: 'you',
		summary: '',
		overridable: true,
		status: 'active',
		body: '',
		level,
		...(scope ? { scope } : {}),
		location: { level, ...(scope ? { scope } : {}) },
		author: 'you',
		approvedBy: 'you',
		...rest
	};
};

const BRANCH_AND_MERGE = `# Branch, review and merge

1. Keep one open branch at a time: merge it before you start new work.
2. Ask the reviewer for READY on the exact commit.
3. Wait for the lead's go, then merge with --no-ff.
4. Clean up the branch after the merge.`;

export const SAMPLE_TEAMS: TeamRef[] = [
	{ id: 'eng', name: 'Engineering', lead_member_id: 'sam' },
	{ id: 'web', name: 'Web', owner_team_id: 'eng', lead_member_id: 'noor' },
	{ id: 'platform', name: 'Platform', owner_team_id: 'eng', lead_member_id: 'kai' },
	{ id: 'design', name: 'Design', lead_member_id: 'juno' },
	{ id: 'people', name: 'People', lead_member_id: 'hana' }
];

export const SAMPLE_PROJECTS: ProjectRef[] = [
	{ key: 'atl', name: 'Atlas', teamIds: ['platform'] },
	{ key: 'bea', name: 'Beacon', teamIds: ['web', 'design'] },
	{ key: 'onb', name: 'Onboarding kit', teamIds: ['people'] },
	{ key: 'scr', name: 'Scratchpad', teamIds: [] }
];

export function sampleProcessDocs(): ProcessDoc[] {
	seq = 0;
	return [
		// Organization
		doc({
			key: 'branch-and-merge',
			name: 'Branch, review and merge',
			level: 'organization',
			version: 3,
			owner: 'sam',
			summary: 'One open branch; review; the lead’s go; merge; clean up.',
			appliesTo: 'every agent that changes a repository',
			body: BRANCH_AND_MERGE,
			moments: { starting_task: 'Check that your open branch is merged before you start new work.', waking_up: 'If your branch is waiting for review or the go, check on it first.', finishing_task: 'Ask the reviewer for READY on the exact commit.' },
			checklist: [
				{ text: 'Own fresh review', required: true },
				{ text: 'Reviewer’s READY for the exact commit', required: true },
				{ text: 'The lead’s go', required: true }
			]
		}),
		doc({ key: 'code-review', name: 'Code review', level: 'organization', version: 2, summary: 'Two sets of eyes on every change; small diffs; say what you checked.', body: '# Code review\n\nReview the diff, not the author. Say what you checked.' }),
		doc({ key: 'weekly-status', name: 'Weekly status', level: 'organization', version: 1, summary: 'A short status on Friday: done, next, blocked.', body: '# Weekly status\n\nPost done, next and blocked on Friday.' }),
		doc({ key: 'security-basics', name: 'Security basics', level: 'organization', version: 1, overridable: false, summary: 'No secrets in the repository; review dependencies before adding them.', body: '# Security basics\n\nNever commit secrets.' }),
		// Teams
		doc({ key: 'incident-response', name: 'Incident response', level: 'team', scope: 'eng', version: 1, summary: 'Page the on-call lead; write the timeline as you go.', body: '# Incident response\n\nPage the on-call lead.' }),
		doc({ key: 'code-review', name: 'Code review (Web)', level: 'team', scope: 'web', version: 2, overrides: { key: 'code-review', version: 1 }, summary: 'Web changes also need a visual check.', body: '# Code review (Web)\n\nAs the organization says, plus a screenshot of any UI change.' }),
		doc({ key: 'code-review', name: 'Code review (Design)', level: 'team', scope: 'design', version: 1, overrides: { key: 'code-review', version: 2 }, summary: 'Design reviews happen in the design tool.', body: '# Code review (Design)\n\nReview in the design tool.' }),
		doc({ key: 'security-basics', name: 'Security basics (Platform)', level: 'team', scope: 'platform', version: 1, overrides: { key: 'security-basics', version: 1 }, summary: 'Platform wants a looser rule.', body: '# Security basics (Platform)' }),
		// Projects
		doc({ key: 'release-checklist', name: 'Release checklist', level: 'project', scope: 'atl', version: 1, summary: 'Tag, changelog, announce.', body: '# Release checklist\n\nTag the release, write the changelog, announce it.', checklist: [{ text: 'Changelog written', required: true }] }),
		doc({ key: 'weekly-status', name: 'Weekly status', level: 'project', scope: 'atl', version: 1, status: 'off', reason: 'we report daily', summary: 'Turned off for Atlas.', body: '', author: 'sam', approvedBy: 'you' }),
		doc({ key: 'branch-and-merge', name: 'Branch, review and merge', level: 'project', scope: 'onb', version: 1, status: 'off', reason: 'a single agent works here', summary: 'Turned off for Onboarding kit.', body: '', author: 'hana', approvedBy: 'hana' }),
		// A file that cannot be read, and two files with one key
		doc({ key: 'design-handoff', name: 'Design handoff', level: 'team', scope: 'design', version: 1, summary: 'Hand designs over with the states listed.', body: '# Design handoff', problems: [{ field: 'overridable', message: 'must be true or false' }] }),
		doc({ key: 'standup', name: 'Standup', level: 'project', scope: 'bea', version: 1, summary: 'A short standup.', body: '# Standup' }),
		doc({ key: 'standup', name: 'Standup notes', level: 'project', scope: 'bea', version: 1, summary: 'Another file with the same key.', body: '# Standup notes' }),
		// An agent's own
		doc({ key: 'my-notes', name: 'My notes', level: 'agent', scope: 'noor', version: 1, summary: 'I keep a running log while I work.', body: '# My notes' }),
		doc({ key: 'code-review', name: 'My code review', level: 'agent', scope: 'noor', version: 1, summary: 'My own way of reviewing.', body: '# My code review' })
	];
}

/** Who may approve at each level in the sample: the owner, and the leads of the project's teams. */
export const approvalFor = (project: ProjectRef): ApprovalContext => ({
	ownerId: 'you',
	teams: SAMPLE_TEAMS,
	projectLeads: project.teamIds.flatMap((id) => SAMPLE_TEAMS.find((t) => t.id === id)?.lead_member_id ?? [])
});
