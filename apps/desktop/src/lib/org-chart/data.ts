// Sample organization for the org chart preview. Invented people, not read from any workspace.
// The real data will come from the office server; its contract (org-proto) doesn't define the
// org chart yet, so these types are local to the app for now.

export type AgentStatus = 'working' | 'idle' | 'offline';

export interface OrgNode {
	id: string;
	name: string;
	role: string;
	/** Absent for the owner, who is a person rather than an agent. */
	status?: AgentStatus;
	/** Open tasks. */
	tasks?: number;
	children?: OrgNode[];
}

export const SAMPLE_ORG: OrgNode = {
	id: 'owner',
	name: 'You',
	role: 'Owner',
	children: [
		{
			id: 'avery',
			name: 'Avery Stone',
			role: 'Chief of Staff',
			status: 'working',
			tasks: 4,
			children: [
				{
					id: 'sam',
					name: 'Sam Okafor',
					role: 'Staff Engineer',
					status: 'working',
					tasks: 6,
					children: [
						{ id: 'noor', name: 'Noor Haddad', role: 'Frontend Engineer', status: 'working', tasks: 3 },
						{ id: 'kai', name: 'Kai Lindqvist', role: 'Backend Engineer', status: 'idle', tasks: 2 },
						{ id: 'mira', name: 'Mira Castillo', role: 'QA Engineer', status: 'offline', tasks: 0 }
					]
				},
				{
					id: 'juno',
					name: 'Juno Park',
					role: 'Product Designer',
					status: 'idle',
					tasks: 2,
					children: [
						{ id: 'pax', name: 'Pax Moreau', role: 'UX Researcher', status: 'working', tasks: 1 }
					]
				},
				{
					id: 'hana',
					name: 'Hana Weiss',
					role: 'Head of People',
					status: 'idle',
					tasks: 1,
					children: [
						{ id: 'leo', name: 'Leo Brandt', role: 'Recruiter', status: 'offline', tasks: 0 },
						{ id: 'ines', name: 'Inês Duarte', role: 'Onboarding', status: 'idle', tasks: 1 }
					]
				}
			]
		}
	]
};

/** Every node in the tree, parents before children. */
export function flatten(node: OrgNode): OrgNode[] {
	return [node, ...(node.children ?? []).flatMap(flatten)];
}

/** Ids of every node that has reports, so a chart can collapse them all. */
export function parentIds(node: OrgNode): string[] {
	return flatten(node)
		.filter((n) => (n.children?.length ?? 0) > 0)
		.map((n) => n.id);
}

/** All reports below `node`, direct and indirect. */
export function reportCount(node: OrgNode): number {
	return flatten(node).length - 1;
}

export function initialsOf(name: string): string {
	const parts = name.trim().split(/\s+/).filter(Boolean);
	if (parts.length === 0) return '?';
	const first = [...parts[0]][0];
	const last = parts.length > 1 ? [...parts[parts.length - 1]][0] : '';
	return (first + last).toUpperCase();
}
