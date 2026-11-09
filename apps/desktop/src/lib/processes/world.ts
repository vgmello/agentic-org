import { SAMPLE_PROJECTS, SAMPLE_TEAMS, approvalFor, sampleProcessDocs } from './sample';
import type { ApprovalContext, ProcessDoc, ProjectRef, TeamRef } from './types';

// The world the Processes screens read, until HQ serves processes: the sample organization's files, teams, projects and agents.
// It is Sample data and says so on every screen.

export interface AgentRef {
	id: string;
	name: string;
	/** The project of the agent's current work; none: no current work (an on-call or scheduled agent waking with no task). */
	workingOn?: string;
	/** The version of each process it last read, with when. */
	reads: Record<string, { docId: string; version: number; at: string }>;
}

export interface ProcessWorld {
	docs: ProcessDoc[];
	teams: TeamRef[];
	projects: ProjectRef[];
	agents: AgentRef[];
	ownerId: string;
	/** When the handbook repository was last read (the last copy is what shows while it cannot be reached). */
	handbookCopyAt: string;
	approval: (project?: ProjectRef) => ApprovalContext;
}

export function sampleWorld(): ProcessWorld {
	const docs = sampleProcessDocs();
	const read = (key: string, at: string, version?: number) => {
		const doc = docs.find((d) => d.key === key && d.level === 'organization');
		return doc ? { docId: doc.id, version: version ?? doc.version, at } : undefined;
	};
	return {
		docs,
		teams: SAMPLE_TEAMS,
		projects: SAMPLE_PROJECTS,
		ownerId: 'you',
		handbookCopyAt: '2026-11-08T09:12:00.000Z',
		approval: (project) => approvalFor(project ?? { key: '', name: '', teamIds: [] }),
		agents: [
			{ id: 'noor', name: 'Noor Haddad', workingOn: 'bea', reads: { 'branch-and-merge': { ...read('branch-and-merge', '2026-11-08T09:12:00.000Z', 2)!, version: 2 }, 'code-review': read('code-review', '2026-11-08T09:12:00.000Z')! } },
			{ id: 'kai', name: 'Kai Lindqvist', workingOn: 'atl', reads: {} },
			{ id: 'sam', name: 'Sam Okafor', reads: {} },
			{ id: 'juno', name: 'Juno Park', workingOn: 'bea', reads: {} }
		]
	};
}
