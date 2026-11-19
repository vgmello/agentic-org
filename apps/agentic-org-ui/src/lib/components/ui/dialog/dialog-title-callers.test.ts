import { readFileSync } from 'node:fs';
import { parse } from 'svelte/compiler';
import { render } from 'svelte/server';
import { expect, test } from 'vitest';
import Fixture from './dialog-title.test.svelte';

type Node = { type: string; start: number; end: number; [key: string]: unknown };
function nodes(value: unknown): Node[] {
	if (!value || typeof value !== 'object') return [];
	const children = Object.values(value).flatMap(nodes);
	return 'type' in value && typeof value.type === 'string' ? [value as Node, ...children] : children;
}

// These are compatibility expressions, not native coverage of absent caller models.
// Parse the checked-in caller so removing its ?? '' fails the absent-value case.
const cases: [string, number, string, string, boolean?][] = [
	['components/ChangeManagerDialog.svelte', 0, 'Change manager of Name', 'Change manager of '],
	['components/DeleteTeamDialog.svelte', 0, 'Delete Team?', 'Delete ?'],
	['components/DeleteTeamDialog.svelte', 0, "Can't delete Team", "Can't delete ", true],
	['components/EditMemberTeamsDialog.svelte', 0, 'Teams for Name', 'Teams for '],
	['components/EditMembersDialog.svelte', 0, 'Members of Name', 'Members of '],
	['components/EditPersonDialog.svelte', 0, 'Edit Name', 'Edit '],
	['components/MoveMemberDialog.svelte', 0, 'Move Name to a team', 'Move  to a team'],
	['components/MoveTeamDialog.svelte', 0, 'Move Name to…', 'Move  to…'],
	['components/NewRequestDialog.svelte', 0, 'New request', 'New request'],
	['components/NewTaskDialog.svelte', 0, 'New task', 'New task'],
	['components/ProjectTeamDialog.svelte', 0, 'Add a project to Team', 'Add a project to '],
	['components/ProjectTeamDialog.svelte', 1, 'Assign a team to Project', 'Assign a team to '],
	['components/RemovePersonDialog.svelte', 0, 'Remove Name from Acme?', 'Remove  from ?'],
	['components/RemoveProjectDialog.svelte', 0, 'Remove Project from Team?', 'Remove  from ?'],
	['components/RenameTeamDialog.svelte', 0, 'Rename Title', 'Rename Title'],
	['components/ReportBugDialog.svelte', 0, 'Report a bug', 'Report a bug'],
	['components/TasksBoard.svelte', 0, 'Block T-1', 'Block T-1'],
	['modules/employees/features/configure-run/HowItRunsDialog.svelte', 0, 'How Name runs', 'How  runs'],
	['modules/employees/features/edit-profile/EditAgentDialog.svelte', 0, 'Edit Name', 'Edit '],
	['modules/employees/features/offboard-employee/OffboardAgentDialog.svelte', 0, 'Offboard Name?', 'Offboard ?'],
	['modules/employees/features/pause-employee/PauseAgentDialog.svelte', 0, 'Pause Name?', 'Pause ?'],
	['modules/messaging/features/manage-channel/ChannelDialog.svelte', 0, 'Title', 'Title'],
	['modules/messaging/features/open-dm/NewMessageDialog.svelte', 0, 'New message', 'New message'],
	['modules/messaging/features/run-command/CommandDialog.svelte', 0, 'Command title', 'Command title']
];

function heading(text: string) {
	const body = render(Fixture, { props: { text } }).body;
	const headings = [...body.matchAll(/<div\b[^>]*role="heading"[^>]*>([\s\S]*?)<\/div>/g)];
	expect(headings).toHaveLength(1);
	return headings[0][1].replace(/<!--[\s\S]*?-->/g, '').replace(/<\/?span\b[^>]*>/g, '');
}

for (const [file, site, present, absent, refused = false] of cases) {
	test(`caller title compatibility ${file} site ${site} refused ${refused}`, () => {
		const source = readFileSync(new URL(`../../../${file}`, import.meta.url), 'utf8');
		const titles = nodes(parse(source, { modern: true }).fragment).filter(n => n.type === 'Component' && n.name === 'DialogTitle');
		expect(titles.length).toBeGreaterThan(site);
		const attribute = (titles[site].attributes as Node[]).find(n => n.type === 'Attribute' && n.name === 'text');
		expect(attribute?.value).toMatchObject({ type: 'ExpressionTag' });
		const expression = (attribute!.value as Node).expression as Node;
		const exact = source.slice(expression.start, expression.end);
		const evaluate = (expression: string, missing: boolean): string => {
			const scope = {
				shown: { name: missing ? undefined : 'Name', projectName: missing ? undefined : 'Project', teamName: missing ? undefined : 'Team' },
				shownTeam: { name: missing ? undefined : 'Team' }, shownProject: { name: missing ? undefined : 'Project' },
				organization: missing ? undefined : 'Acme', summary: { refused }, title: 'Title', blockTitle: 'T-1', form: { title: 'Command title' }
			};
			// Only the expression parsed from trusted repository source is executable.
			return new Function(...Object.keys(scope), `return (${expression});`)(...Object.values(scope));
		};
		expect(heading(evaluate(exact, false))).toBe(present);
		expect(heading(evaluate(exact, true))).toBe(absent);
		// DeleteTeam has two mutually exclusive template branches. Mutate the branch
		// this row actually evaluates; the other row covers the opposite branch.
		const active = expression.type === 'ConditionalExpression' ? (refused ? expression.consequent : expression.alternate) : expression;
		for (const fallback of nodes(active).filter(n => n.type === 'LogicalExpression' && n.operator === '??')) {
			const left = fallback.left as Node;
			const mutant = exact.slice(0, fallback.start - expression.start) + source.slice(left.start, left.end) + exact.slice(fallback.end - expression.start);
			// Delete one actual caller coalescer at a time, preserving the other sites.
			const rendered = heading(evaluate(mutant, true));
			expect(rendered).toContain('undefined');
			expect(rendered).not.toBe(absent);
		}
	});
}

test('compatibility table covers all 22 migrated files and 23 title sites', () => {
	expect(new Set(cases.map(([file]) => file)).size).toBe(22);
	expect(new Set(cases.map(([file, site]) => `${file}:${site}`)).size).toBe(23);
});
