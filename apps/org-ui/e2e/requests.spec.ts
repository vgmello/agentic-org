import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function open(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
}
const queue = async (page: Page) => {
	await visit(page, '/work');
	await page.getByRole('tab', { name: /^Requests/ }).click();
	await expect(page.getByTestId('requests-queue')).toBeVisible();
};
async function capture(page: Page, text: string, opts: { kind?: string; project?: string; button?: RegExp } = {}) {
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	await expect(dialog.getByLabel('What do you need?')).toBeFocused();
	await dialog.getByLabel('What do you need?').fill(text);
	if (opts.kind) await dialog.getByLabel('Kind').selectOption({ label: opts.kind });
	if (opts.project) await dialog.getByLabel('Project').selectOption({ label: opts.project });
	await dialog.getByRole('button', { name: opts.button ?? /^(Send to|Save)/ }).click();
	await expect(dialog).toBeHidden();
}
/** An agent working through the CLI as the member the request was sent to: accept, then scope with an outcome. */
async function agentScopes(page: Page, requestTitle: string, draftTeam?: string) {
	await page.evaluate(
		async ([title, team]) => {
			const saved = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]') as { organization: { id: string } }[];
			const org = saved[saved.length - 1].organization.id;
			const w = window as unknown as { __mockWorkAs: (o: string, m: string, c: object) => Promise<{ items: { id: string; version: number }[] }> };
			const state = JSON.parse(localStorage.getItem(`agentic-org.mock.work.${org}`) ?? '{}') as { items: Record<string, { summary: { id: string; title: string; version: number; assignee_id: string } }> };
			const request = Object.values(state.items).find((i) => i.summary.title === title)!.summary;
			const accepted = await w.__mockWorkAs(org, request.assignee_id, { method: 'request.accept', change_id: `01JC000000000000000000A${request.version}`, item_id: request.id, expected_version: request.version });
			await w.__mockWorkAs(org, request.assignee_id, {
				method: 'request.scope',
				change_id: '01JC000000000000000000B001',
				item_id: request.id,
				expected_version: accepted.items[0].version,
				outcome: {
					docs: [],
					feature: { type: 'new', feature_id: '01JC000000000000000000F001' },
					draft_tasks: [
						{ draft_id: '01JC000000000000000000D001', title: 'Design the flow' },
						{ draft_id: '01JC000000000000000000D002', title: 'Build the screen', ...(team ? { team_id: team } : {}) }
					],
					estimate: { effort: '2 days', cost_usd_cents_min: 4000, cost_usd_cents_max: 7000 }
				}
			});
		},
		[requestTitle, draftTeam]
	);
}

test('RQS-7: with no requests the queue says so, and Requests is a pill of Work', async ({ page }) => {
	await open(page);
	await queue(page);
	await expect(page.getByTestId('no-requests')).toBeVisible();
	await expect(page.getByRole('tab', { name: 'Debt' })).toHaveAttribute('aria-disabled', 'true');
	// The pills run Tasks, Bugs, Debt, Requests.
	await expect(page.getByRole('tab')).toHaveText([/^Tasks/, /^Bugs/, /^Debt/, /^Requests/]);
	await expect(page.getByText('Requests across every project.')).toBeVisible();
});

test('RQS-2/3/5: the words are enough, the route is suggested by rule with a reason and can be changed, and it lands With agents', { tag: '@journey' }, async ({ page }) => {
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	await dialog.getByLabel('What do you need?').fill('Let me capture requests with a shortcut and route them');
	await expect(dialog.getByTestId('by-rule')).toHaveText('Suggested by rule');
	const radios = dialog.getByRole('radio');
	await expect(radios.first()).toBeChecked();
	await shot(page, 'request-capture');
	// Every suggestion says why, and changing the choice drops "Suggested by rule".
	// The last radio is Someone else; the others are suggestions.
	const suggested = (await radios.count()) - 1;
	for (let i = 0; i < suggested; i++) await expect(radios.nth(i).locator('xpath=ancestor::label')).toContainText(/, .+: .+/);
	await expect(dialog.getByRole('radio', { name: /Don't send/ })).toHaveCount(0);
	await dialog.getByRole('radio').nth(1).check();
	await expect(dialog.getByTestId('by-rule')).toHaveCount(0);
	await expect(dialog.getByRole('button', { name: /^Send to / })).toBeVisible();
	await dialog.getByRole('radio').first().check();
	await dialog.getByRole('button', { name: /^Send to/ }).click();
	await expect(dialog).toBeHidden();
	const group = page.locator('[data-group=with_agents]');
	await expect(group).toContainText('Let me capture requests');
	await expect(group.getByTestId('request-words-state')).toContainText('not accepted yet');
	await shot(page, 'requests-queue-routed');
});

test('RQS-2: Just save queues it unrouted, empty words are refused, and Route sends it later', async ({ page }) => {
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	await dialog.getByRole('button', { name: 'Just save' }).click();
	await expect(dialog.getByText('Write what you need.')).toBeVisible();
	await dialog.getByLabel('What do you need?').fill('Dark mode for the docs reader');
	await dialog.getByRole('button', { name: 'Just save' }).click();
	await expect(dialog).toBeHidden();
	const group = page.locator('[data-group=not_routed]');
	await expect(group).toContainText('Dark mode for the docs reader');
	await group.getByRole('button', { name: /^Route REQ-/ }).click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByTestId('request-words')).toHaveText('Dark mode for the docs reader');
	await sheet.getByLabel('Who scopes it').selectOption({ index: 1 });
	await sheet.getByRole('button', { name: 'Send', exact: true }).click();
	await expect(sheet.getByTestId('request-state')).toContainText('not accepted yet');
	await expect(sheet.getByTestId('request-history')).toContainText('You sent this to');
});

test('RQS-9/14: scoped by an agent it needs you; Approve asks for a project, lets the drafts be edited, and makes the tasks', { tag: '@journey' }, async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Costs per project', { kind: 'Feature' });
	await agentScopes(page, 'Costs per project');
	await visit(page, '/work');
	await expect(page.getByTestId('needs-you')).toHaveText('1 item needs you');
	await page.getByRole('tab', { name: /^Requests/ }).click();
	await expect(page.getByTestId('requests-badge')).toHaveText('1');
	const group = page.locator('[data-group=needs_you]');
	await expect(group.getByTestId('request-words-state')).toHaveText(/^Scoped by /);
	await group.getByRole('button', { name: /^Review REQ-/ }).click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByTestId('request-estimate')).toHaveText('Estimate: 2 days · ≈ $40–70');
	await expect(sheet.getByTestId('draft-task')).toHaveText(['Design the flow', 'Build the screen']);
	await expect(sheet.getByTestId('request-outcome')).toContainText('A new feature: Costs per project');
	// With one open request there is nothing to merge into, and the page says so.
	await expect(sheet.getByRole('button', { name: 'Merge into…' })).toBeDisabled();
	await expect(sheet.getByTestId('no-merge')).toHaveText('No other open requests to merge into.');
	// Scoped: approve, ask for changes, merge or decline; the route is not changed once it is scoped.
	await expect(sheet.getByRole('button', { name: /^(Route|Change who scopes it)/ })).toHaveCount(0);
	await shot(page, 'request-scoped');
	await sheet.getByRole('button', { name: /^Approve and create 2 tasks/ }).click();
	// No project yet: it asks which, and nothing is created without one.
	await expect(sheet.getByLabel('Which project?')).toBeVisible();
	await expect(sheet.getByRole('button', { name: 'Create 2 tasks' })).toBeDisabled();
	await sheet.getByLabel('Which project?').selectOption({ label: 'Atlas' });
	await sheet.getByLabel('Task 2 title').fill('Build the costs screen');
	await sheet.getByLabel('Create task 1').uncheck();
	await expect(sheet.getByRole('button', { name: 'Create 1 task' })).toBeEnabled();
	await sheet.getByLabel('Create task 1').check();
	await sheet.getByRole('button', { name: 'Create 2 tasks' }).click();
	await expect(sheet.getByTestId('request-state')).toHaveText('Approved');
	await expect(sheet.getByTestId('request-tasks')).toContainText('Tasks created · 0 of 2 done');
	await expect(sheet.getByTestId('request-tasks')).toContainText('Build the costs screen');
	// The proposed list is not repeated once the tasks exist.
	await expect(sheet.getByTestId('proposed-as-approved')).toHaveText('Proposed: 2 tasks (the ones you approved are below).');
	await expect(sheet.getByTestId('draft-task')).toHaveCount(0);
	await expect(sheet.getByTestId('request-history')).toContainText('You approved it');
	await shot(page, 'request-approved');
	await page.keyboard.press('Escape');
	await expect(page.getByTestId('needs-you')).toHaveText('Nothing needs you.');
	// The tasks are in the project (Work opens on the tab it was left on, Requests).
	await visit(page, '/work');
	await page.getByRole('tab', { name: 'Tasks' }).click();
	await expect(page.getByTestId('work-row')).toHaveCount(2);
	await expect(page.getByTestId('work-project')).toHaveText(['Atlas', 'Atlas']);
});

test('RQS-10: asking for changes sends it back to the agent with the note; declining needs a reason; both are in the history', { tag: '@journey' }, async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Weekly digest by email', { project: 'Atlas' });
	await agentScopes(page, 'Weekly digest by email');
	await visit(page, '/work');
	await page.getByRole('tab', { name: /^Requests/ }).click();
	await page.locator('[data-group=needs_you]').getByRole('button', { name: /^Review REQ-/ }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByRole('button', { name: 'Ask for changes…' }).click();
	await expect(sheet.getByRole('button', { name: 'Send back' })).toBeDisabled();
	await sheet.getByLabel('What should change').fill('Daily or weekly?');
	await sheet.getByRole('button', { name: 'Send back' }).click();
	await expect(sheet.getByTestId('request-state')).toHaveText(/ is scoping$/);
	await expect(sheet.getByTestId('request-history')).toContainText('Daily or weekly?');
	// The note is shown once, on the asked-for-changes line.
	await expect(sheet.getByTestId('request-history').getByText('Daily or weekly?')).toHaveCount(1);
	await expect(sheet.getByTestId('request-history')).toContainText('You asked for changes');
	await sheet.getByRole('button', { name: 'Decline…' }).click();
	await expect(sheet.getByRole('button', { name: 'Decline', exact: true })).toBeDisabled();
	await sheet.getByLabel('Why').fill('Out of scope for now');
	await sheet.getByRole('button', { name: 'Decline', exact: true }).click();
	await expect(sheet.getByTestId('decline-reason')).toHaveText('Declined: Out of scope for now');
	await expect(sheet.getByTestId('request-actions')).toHaveCount(0);
	await page.keyboard.press('Escape');
	// The queue opens on what is still going; All shows what ended.
	await expect(page.locator('[data-group=closed]')).toHaveCount(0);
	await page.getByLabel('State').selectOption('all');
	await page.locator('[data-group=closed]').locator('summary').click();
	await expect(page.locator('[data-group=closed]')).toContainText('Declined');
});

test('merging a request into another quotes its words there', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Dark mode please', { button: /^Just save$/ });
	await capture(page, 'Dark mode for the docs reader', { button: /^Just save$/ });
	await page.getByRole('button', { name: 'Dark mode please' }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByRole('button', { name: 'Merge into…' }).click();
	await sheet.getByLabel('Merge into').selectOption({ index: 1 });
	await sheet.getByRole('button', { name: 'Merge', exact: true }).click();
	await expect(sheet.getByTestId('merged-into')).toContainText('Dark mode for the docs reader');
	await page.keyboard.press('Escape');
	await page.getByRole('button', { name: 'Dark mode for the docs reader' }).click();
	await expect(page.getByRole('dialog').getByTestId('request-history')).toContainText('Dark mode please');
});

test('RQS-2: Esc with words asks "Keep as a draft?": Back returns to the words, Discard drops them, Keep saves them unrouted', async ({ page }) => {
	await open(page);
	await queue(page);
	const dialog = page.getByRole('dialog', { name: 'New request' });
	// Nothing typed: Esc just closes.
	await page.getByTestId('new-request').click();
	await page.keyboard.press('Escape');
	await expect(dialog).toBeHidden();
	// Words typed: it asks, and Esc again goes back.
	await page.getByTestId('new-request').click();
	await dialog.getByLabel('What do you need?').fill('Keyboard shortcuts cheat sheet');
	await page.keyboard.press('Escape');
	await expect(dialog.getByTestId('keep-draft')).toHaveText('Keep as a draft?');
	await shot(page, 'request-keep-draft');
	await page.keyboard.press('Escape');
	await expect(dialog.getByTestId('keep-draft')).toHaveCount(0);
	await expect(dialog.getByLabel('What do you need?')).toHaveValue('Keyboard shortcuts cheat sheet');
	await expect(dialog.getByLabel('What do you need?')).toBeFocused();
	// Discard drops the words.
	await dialog.getByRole('button', { name: 'Cancel' }).click();
	await dialog.getByRole('button', { name: 'Discard' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByTestId('no-requests')).toBeVisible();
	// Keep saves them as a request that is not routed.
	await page.getByTestId('new-request').click();
	await dialog.getByLabel('What do you need?').fill('Keyboard shortcuts cheat sheet');
	await page.keyboard.press('Escape');
	await dialog.getByRole('button', { name: 'Keep' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.locator('[data-group=not_routed]')).toContainText('Keyboard shortcuts cheat sheet');
});

test('RQS-3: tabbing through the choices never changes who it is sent to; only choosing does', async ({ page }) => {
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	await dialog.getByLabel('What do you need?').fill('Tab through me');
	const send = dialog.getByRole('button', { name: /^Send to / });
	const label = await send.textContent();
	await dialog.locator('select[aria-label="Someone else"]').focus();
	await page.keyboard.press('Tab');
	await page.keyboard.press('Shift+Tab');
	await expect(send).toHaveText(label!);
	await expect(dialog.getByTestId('by-rule')).toBeVisible();
	// Choosing someone does switch the choice to them, and the rule's tag goes.
	await dialog.locator('select[aria-label="Someone else"]').selectOption({ index: 2 });
	await expect(dialog.getByRole('radio', { name: /^Someone else/ })).toBeChecked();
	await expect(dialog.getByTestId('by-rule')).toHaveCount(0);
});

test('RQS-9: a draft that is removed is not created, and the sheet opens clean for the next request', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Costs per project');
	await capture(page, 'Another idea', { button: /^Just save$/ });
	await agentScopes(page, 'Costs per project');
	await visit(page, '/work');
	await page.getByRole('tab', { name: /^Requests/ }).click();
	await page.getByRole('button', { name: /^Review REQ-1/ }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByRole('button', { name: /^Approve and create 2 tasks/ }).click();
	await sheet.getByLabel('Which project?').selectOption({ label: 'Atlas' });
	await sheet.getByLabel('Create task 1').uncheck();
	await sheet.getByRole('button', { name: 'Create 1 task' }).click();
	await expect(sheet.getByTestId('request-state')).toHaveText('Approved');
	await expect(sheet.getByTestId('request-tasks')).toContainText('Tasks created · 0 of 1 done');
	await expect(sheet.getByTestId('request-tasks')).not.toContainText('Design the flow');
	await page.keyboard.press('Escape');
	// Another request opens with no step begun and nothing carried over.
	await page.getByRole('button', { name: 'Another idea' }).click();
	await expect(sheet.getByTestId('request-step')).toHaveCount(0);
	await expect(sheet.getByTestId('request-words')).toHaveText('Another idea');
	await expect(sheet.getByTestId('request-tasks')).toHaveCount(0);
});

test('RQS-9: a draft that names a team the chosen project does not have is created without it, and the sheet says so', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Costs per project');
	await agentScopes(page, 'Costs per project', 'a-team-that-is-not-there');
	await visit(page, '/work');
	await page.getByRole('tab', { name: /^Requests/ }).click();
	await page.getByRole('button', { name: /^Review REQ-1/ }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByRole('button', { name: /^Approve and create 2 tasks/ }).click();
	await sheet.getByLabel('Which project?').selectOption({ label: 'Atlas' });
	await expect(sheet.getByTestId('drafts-dropped')).toContainText("1 draft names a team or person that doesn't apply there");
	// It says which task loses what.
	await expect(sheet.getByTestId('draft-without-team')).toHaveCount(1);
	await expect(sheet.getByRole('listitem').filter({ hasText: 'loses its team' }).getByLabel('Task 2 title')).toBeVisible();
	await sheet.getByRole('button', { name: 'Create 2 tasks' }).click();
	await expect(sheet.getByTestId('request-state')).toHaveText('Approved');
	await expect(sheet.getByTestId('request-tasks')).toContainText('Tasks created · 0 of 2 done');
});

test('the route a person picks on the page is recorded as theirs, not the rule\'s', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Dark mode', { button: /^Just save$/ });
	await page.getByRole('button', { name: /^Route REQ-1/ }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByLabel('Who scopes it').selectOption({ index: 1 });
	await sheet.getByRole('button', { name: 'Send', exact: true }).click();
	await expect(sheet.getByTestId('request-state')).toContainText('not accepted yet');
	// A person's pick carries no rule reason.
	await expect(sheet.getByTestId('route-reason')).toHaveCount(0);
});

test('RQS-5: a route the rule suggested and was left alone is recorded as the rule\'s; one the owner picked is theirs', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Left to the rule'); // the first suggestion, untouched
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	await dialog.getByLabel('What do you need?').fill('Picked by me');
	await dialog.getByRole('radio').nth(1).check();
	await dialog.getByRole('button', { name: /^Send to / }).click();
	await expect(dialog).toBeHidden();
	await page.getByRole('button', { name: 'Left to the rule' }).click();
	await expect(page.getByRole('dialog').getByTestId('route-reason')).toContainText('(suggested by rule)');
	await page.keyboard.press('Escape');
	await page.getByRole('button', { name: 'Picked by me' }).click();
	// A route the person picked carries no rule reason and is not called the rule's.
	await expect(page.getByRole('dialog').getByTestId('route-reason')).toHaveCount(0);
});

test('closing a page with a step half done and opening another request starts that one clean', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'First idea', { button: /^Just save$/ });
	await capture(page, 'Second idea', { button: /^Just save$/ });
	await page.getByRole('button', { name: 'First idea' }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByRole('button', { name: 'Decline…' }).click();
	await sheet.getByLabel('Why').fill('half typed');
	await page.keyboard.press('Escape');
	await page.getByRole('button', { name: 'Second idea' }).click();
	await expect(sheet.getByTestId('request-words')).toHaveText('Second idea');
	await expect(sheet.getByTestId('request-step')).toHaveCount(0);
	await expect(sheet.getByTestId('request-actions')).toBeVisible();
});

test('RQS-4: words that start with @ and a handle go to that agent: the handle is completed from the names, and the route says so', async ({ page }) => {
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	await dialog.getByLabel('What do you need?').fill('@');
	await expect(dialog.getByTestId('at-matches').getByRole('option')).not.toHaveCount(0);
	await dialog.getByLabel('What do you need?').fill('@sa');
	await dialog.getByTestId('at-matches').getByRole('option', { name: /Sam Okafor/ }).click();
	await expect(dialog.getByLabel('What do you need?')).toHaveValue(/^@\S+$/);
	await dialog.getByLabel('What do you need?').pressSequentially(' fix the build time');
	await expect(dialog.getByTestId('by-at')).toContainText('You wrote @');
	await expect(dialog.getByRole('radio', { name: /Sam Okafor/ })).toBeChecked();
	await expect(dialog.getByRole('button', { name: 'Send to Sam Okafor' })).toBeVisible();
	await shot(page, 'request-at-name');
	await dialog.getByRole('button', { name: 'Send to Sam Okafor' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.locator('[data-group=with_agents]').getByTestId('request-words-state')).toHaveText('Sent to Sam Okafor · not accepted yet');
	// Picking someone else in the list wins over the @name.
	await page.getByTestId('new-request').click();
	await dialog.getByLabel('What do you need?').fill('@sam another one');
	await dialog.getByRole('radio').nth(1).check();
	await expect(dialog.getByTestId('by-at')).toHaveCount(0);
	await expect(dialog.getByRole('button', { name: /^Send to / })).not.toHaveText('Send to Sam Okafor');
});

test('RQS-13: while typing, open requests that read alike are offered, and Add to REQ-n puts the words there instead', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Dark mode for the docs reader', { button: /^Just save$/ });
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	await dialog.getByLabel('What do you need?').fill('I would like a dark mode when reading the docs');
	await expect(dialog.getByTestId('similar')).toContainText('Looks like REQ-1: Dark mode for the docs reader');
	await dialog.getByRole('button', { name: 'Add to REQ-1' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByText('Added to REQ-1.')).toBeVisible();
	await expect(page.getByTestId('request-row')).toHaveCount(1); // no new request
	await page.getByRole('button', { name: 'Dark mode for the docs reader' }).click();
	await expect(page.getByRole('dialog').getByTestId('request-history')).toContainText('I would like a dark mode when reading the docs');
	// Words that read like nothing offer nothing.
	await page.keyboard.press('Escape');
	await page.getByTestId('new-request').click();
	await dialog.getByLabel('What do you need?').fill('Export the whole organization chart as a PDF file');
	await page.waitForTimeout(700); // longer than the pause before the office is asked
	await expect(dialog.getByTestId('similar')).toHaveCount(0);
});

test('RQS-7: the queue can be filtered by state, who scopes it and project, and searched', async ({ page }) => {
	await open(page);
	await queue(page);
	await expect(page.getByTestId('request-filters')).toHaveCount(0); // nothing to filter yet
	await capture(page, 'Dark mode for the docs reader', { button: /^Just save$/, project: 'Atlas' });
	await capture(page, 'Weekly digest by email');
	await capture(page, 'Checklists on tasks', { button: /^Just save$/ });
	const rows = page.getByTestId('request-row');
	await expect(rows).toHaveCount(3);
	await expect(page.getByTestId('request-count')).toHaveText('3 requests');
	await page.getByLabel('Search requests').fill('digest');
	await expect(rows).toHaveCount(1);
	await expect(page.getByTestId('request-count')).toHaveText('1 of 3');
	await page.getByRole('button', { name: 'Clear', exact: true }).click();
	await expect(rows).toHaveCount(3);
	await page.getByLabel('State').selectOption({ label: 'Not routed' });
	await expect(rows).toHaveCount(2);
	await page.getByLabel('State').selectOption({ label: 'All' });
	await page.getByTestId('request-filters').getByLabel('Project').selectOption({ label: 'Atlas' });
	await expect(rows).toHaveCount(1);
	await expect(rows.first()).toContainText('Dark mode');
	await page.getByLabel('Search requests').fill('nothing like this');
	await expect(page.getByTestId('no-request-match')).toBeVisible();
	await page.getByTestId('no-request-match').getByRole('button', { name: 'Clear filters' }).click();
	await expect(rows).toHaveCount(3);
	await page.getByLabel('State').selectOption('all');
	await expect(rows).toHaveCount(3);
});

test('RQS-9: a task an approval made says which request it came from', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Costs per project', { project: 'Atlas' });
	await agentScopes(page, 'Costs per project');
	await visit(page, '/work');
	await page.getByRole('tab', { name: /^Requests/ }).click();
	await page.getByRole('button', { name: /^Review REQ-1/ }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByRole('button', { name: /^Approve and create 2 tasks/ }).click();
	await sheet.getByRole('button', { name: 'Create 2 tasks' }).click();
	await expect(sheet.getByTestId('request-state')).toHaveText('Approved');
	await page.keyboard.press('Escape');
	await page.getByRole('tab', { name: 'Tasks' }).click();
	await page.getByRole('button', { name: 'Design the flow' }).click();
	await expect(page.getByRole('dialog').getByTestId('task-from-request')).toHaveText('From REQ-1 · Costs per project');
});

test('RQS-4: an @name to an agent the rule also suggests is the person\'s route (no rule reason), punctuation after it is fine, and typing it after a pick wins', async ({ page }) => {
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	await dialog.getByLabel('What do you need?').fill('Something for later');
	// A pick made first does not stand once an @name is typed after it.
	await dialog.getByRole('radio').nth(1).check();
	await dialog.getByLabel('What do you need?').fill('@pax: please look at this');
	await expect(dialog.getByTestId('by-at')).toContainText('You wrote @pax');
	await expect(dialog.getByRole('button', { name: /^Send to Pax Moreau/ })).toBeVisible();
	await dialog.getByRole('button', { name: /^Send to Pax Moreau/ }).click();
	await expect(dialog).toBeHidden();
	// The saved words drop the @name that routed them.
	await page.getByRole('button', { name: 'please look at this' }).click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByTestId('request-state')).toContainText('Sent to Pax Moreau');
	await expect(sheet.getByTestId('route-reason')).toHaveCount(0); // not the rule's reason, and not called the rule's
});

test('RQS-13: words too long to be a note can not be added to a request, and say so', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Dark mode for the docs reader', { button: /^Just save$/ });
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	await dialog.getByLabel('What do you need?').fill(`I want a dark mode for the docs. ${'More detail. '.repeat(800)}`);
	const add = dialog.getByRole('button', { name: 'Add to REQ-1' });
	await expect(add).toBeDisabled();
	await expect(add).toHaveAttribute('title', /at most/);
});

test('RQS-7: a search whose only match has ended opens the group it is in', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Costs per project');
	await agentScopes(page, 'Costs per project');
	await visit(page, '/work');
	await page.getByRole('tab', { name: /^Requests/ }).click();
	await page.getByRole('button', { name: /^Review REQ-1/ }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByRole('button', { name: /^Approve and create 2 tasks/ }).click();
	await sheet.getByLabel('Which project?').selectOption({ label: 'Atlas' });
	await sheet.getByRole('button', { name: 'Create 2 tasks' }).click();
	await page.keyboard.press('Escape');
	await capture(page, 'Another one', { button: /^Just save$/ });
	await expect(page.locator('[data-group=approved]')).toHaveCount(0); // Open hides what ended
	await page.getByLabel('State').selectOption('all');
	await expect(page.locator('[data-group=approved]')).toHaveJSProperty('open', false);
	await page.getByLabel('Search requests').fill('costs');
	await expect(page.locator('[data-group=approved]')).toHaveJSProperty('open', true);
	await expect(page.locator('[data-group=approved]').getByTestId('request-row')).toBeVisible();
});

test('RQS-4: the handle list works from the keyboard, and the saved words drop the @handle that routed them', async ({ page }) => {
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	const words = dialog.getByLabel('What do you need?');
	await words.fill('@');
	const options = dialog.getByTestId('at-matches').getByRole('option');
	await expect(options.first()).toHaveAttribute('aria-selected', 'true');
	await words.press('ArrowDown');
	await expect(options.nth(1)).toHaveAttribute('aria-selected', 'true');
	await words.press('ArrowUp');
	await expect(options.first()).toHaveAttribute('aria-selected', 'true');
	// Esc closes the list, not the panel.
	await words.press('Escape');
	await expect(dialog.getByTestId('at-matches')).toHaveCount(0);
	await expect(dialog).toBeVisible();
	// Typing again brings it back; Enter picks the highlighted one.
	await words.fill('@sa');
	await expect(options).toHaveCount(1);
	await words.press('Enter');
	await expect(words).toHaveValue(/^@\S+$/);
	await expect(dialog.getByTestId('by-at')).toBeVisible();
	await words.pressSequentially(' fix the build time');
	await dialog.getByRole('button', { name: 'Send to Sam Okafor' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByRole('button', { name: 'fix the build time', exact: true })).toBeVisible();
	await page.getByRole('button', { name: 'fix the build time', exact: true }).click();
	await expect(page.getByRole('dialog').getByTestId('request-words')).toHaveText('fix the build time');
});

test('RQS-4: words that are only an @handle ask for words, and an @name later in the text is just words', async ({ page }) => {
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	const words = dialog.getByLabel('What do you need?');
	await words.fill('@sam');
	await dialog.getByRole('button', { name: 'Send to Sam Okafor' }).click();
	await expect(dialog.getByText('Write what you need after @sam.')).toBeVisible();
	await words.fill('ask @sam later');
	await expect(dialog.getByTestId('by-at')).toHaveCount(0);
	await expect(dialog.getByTestId('by-rule')).toBeVisible();
});

test('RQS-7: the queue can be filtered to what one person raised, and opens on Open', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Mine to keep', { button: /^Just save$/ });
	await expect(page.getByLabel('State')).toHaveValue('open');
	await expect(page.getByLabel('People').locator('option', { hasText: 'Raised by me' })).toHaveCount(1);
	await page.getByLabel('People').selectOption({ label: 'Raised by me' });
	await expect(page.getByTestId('request-row')).toHaveCount(1);
});

test('RQS-4: a handle nobody has says so and turns Send off until it is fixed', async ({ page }) => {
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	const words = dialog.getByLabel('What do you need?');
	await words.fill('@nobody-here do the thing');
	await expect(dialog.getByTestId('no-such-handle')).toHaveText('No one called @nobody-here. Pick someone from the list or remove it.');
	await expect(dialog.locator('button[type=submit]')).toBeDisabled();
	// Without a messaging capability composed (none is yet), there is no Message instead button.
	await expect(dialog.getByRole('button', { name: 'Message instead' })).toHaveCount(0);
	await words.fill('do the thing');
	await expect(dialog.getByTestId('no-such-handle')).toHaveCount(0);
	await expect(dialog.locator('button[type=submit]')).toBeEnabled();
});

test('RQS-15: the filters are remembered for the organization', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Dark mode for the docs reader', { button: /^Just save$/ });
	await capture(page, 'Weekly digest by email', { button: /^Just save$/ });
	await page.getByLabel('Search requests').fill('digest');
	await page.getByLabel('State').selectOption('all');
	await expect(page.getByTestId('request-row')).toHaveCount(1);
	await queue(page); // the page is loaded again
	await expect(page.getByLabel('Search requests')).toHaveValue('digest');
	await expect(page.getByLabel('State')).toHaveValue('all');
	await expect(page.getByTestId('request-row')).toHaveCount(1);
	await page.getByRole('button', { name: 'Clear', exact: true }).click();
	await queue(page);
	await expect(page.getByLabel('Search requests')).toHaveValue('');
	await expect(page.getByLabel('State')).toHaveValue('open');
});

test('RQS-16: From REQ-n on a task links to the request', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Costs per project', { project: 'Atlas' });
	await agentScopes(page, 'Costs per project');
	await visit(page, '/work');
	await page.getByRole('tab', { name: /^Requests/ }).click();
	await page.getByRole('button', { name: /^Review REQ-1/ }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByRole('button', { name: /^Approve and create 2 tasks/ }).click();
	await sheet.getByRole('button', { name: 'Create 2 tasks' }).click();
	await expect(sheet.getByTestId('request-state')).toHaveText('Approved');
	await page.keyboard.press('Escape');
	await page.getByRole('tab', { name: 'Tasks' }).click();
	await page.getByRole('button', { name: 'Design the flow' }).click();
	await page.getByRole('dialog').getByRole('link', { name: 'REQ-1 · Costs per project' }).click();
	await expect(page.getByRole('dialog').getByTestId('request-state')).toHaveText('Approved');
	await expect(page.getByRole('dialog').getByTestId('request-words')).toHaveText('Costs per project');
});

test('RQS-4: the handle list belongs to the handle: it is gone once the caret leaves it, Esc keeps it closed while typing on, and Tab picks', async ({ page }) => {
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	const words = dialog.getByLabel('What do you need?');
	// A half handle with more text after it, the caret at the end: no list, and Enter is a new line.
	await words.fill('@sa fix the build');
	await expect(dialog.getByTestId('at-matches')).toHaveCount(0);
	await words.press('Enter');
	await expect(words).toHaveValue('@sa fix the build\n');
	// The caret back in the handle brings the list; Esc closes it and it stays closed while typing on.
	await words.fill('@sa');
	await expect(dialog.getByTestId('at-matches')).toBeVisible();
	await words.press('Escape');
	await expect(dialog.getByTestId('at-matches')).toHaveCount(0);
	await words.pressSequentially(' ');
	await words.pressSequentially('fix');
	await expect(dialog.getByTestId('at-matches')).toHaveCount(0);
	await expect(dialog).toBeVisible();
	// A different handle brings the list back, and Tab picks the highlighted agent.
	await words.fill('@pa');
	await expect(dialog.getByTestId('at-matches')).toBeVisible();
	await words.press('Tab');
	await expect(words).toHaveValue(/^@\S+$/);
	await expect(dialog.getByTestId('by-at')).toBeVisible();
});

test('RQS-4: a half-typed or shared handle with words after it is not a route: it says so and Send is off until one is picked', async ({ page }) => {
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	const words = dialog.getByLabel('What do you need?');
	await words.fill('@sa fix the build');
	await expect(dialog.getByTestId('no-such-handle')).toContainText('@sa could be Sam Okafor');
	await expect(dialog.locator('button[type=submit]')).toBeDisabled();
	await expect(dialog.locator('button[type=submit]')).toHaveAttribute('aria-describedby', 'handle-problem');
	// Fixing the handle (a full one) clears it; so does picking someone in the list.
	await words.fill('@sam fix the build');
	await expect(dialog.getByTestId('no-such-handle')).toHaveCount(0);
	await words.fill('@sa fix the build');
	await dialog.getByRole('radio').nth(1).check();
	await expect(dialog.getByTestId('no-such-handle')).toHaveCount(0);
	await expect(dialog.locator('button[type=submit]')).toBeEnabled();
});

test('RQS-4: a pick made before an unknown handle is typed does not settle it', async ({ page }) => {
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	const words = dialog.getByLabel('What do you need?');
	await words.fill('do the thing');
	await dialog.getByRole('radio').nth(1).check();
	await words.fill('@nobody-here do the thing');
	await expect(dialog.getByTestId('no-such-handle')).toContainText('No one called @nobody-here');
	await expect(dialog.locator('button[type=submit]')).toBeDisabled();
});

test('RQS-4: choosing someone in the list settles a handle nobody has: the words stay as typed and Send works', async ({ page }) => {
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	await dialog.getByLabel('What do you need?').fill('@nobody-here do the thing');
	await expect(dialog.locator('button[type=submit]')).toBeDisabled();
	await dialog.getByRole('radio').nth(1).check();
	await expect(dialog.getByTestId('no-such-handle')).toHaveCount(0);
	await expect(dialog.locator('button[type=submit]')).toBeEnabled();
	await dialog.locator('button[type=submit]').click();
	await expect(dialog).toBeHidden();
	await expect(page.getByRole('button', { name: '@nobody-here do the thing' })).toBeVisible();
});

test('RQS-15: when nothing is open the queue says so and offers All, instead of a Clear that changes nothing', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Not now');
	await page.getByRole('button', { name: /^Not now$/ }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByRole('button', { name: 'Decline…' }).click();
	await sheet.getByLabel('Why').fill('later');
	await sheet.getByRole('button', { name: 'Decline', exact: true }).click();
	await page.keyboard.press('Escape');
	await expect(page.getByTestId('nothing-open')).toContainText('Nothing is open.');
	await page.getByRole('button', { name: 'Show all requests' }).click();
	await expect(page.getByTestId('request-row')).toHaveCount(1);
});

test('RQS-15: the State filter can show one group, and REQ-1 does not find REQ-12', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Dark mode for the docs reader', { button: /^Just save$/ });
	await capture(page, 'Weekly digest by email');
	await page.getByLabel('State').selectOption({ label: 'Not routed' });
	await expect(page.getByTestId('request-row')).toHaveCount(1);
	await page.getByLabel('State').selectOption({ label: 'With agents' });
	await expect(page.getByTestId('request-row')).toHaveCount(1);
	await expect(page.getByTestId('request-row')).toContainText('Weekly digest');
	await page.getByLabel('State').selectOption({ label: 'Needs you' });
	await expect(page.getByTestId('no-request-match')).toBeVisible();
	await page.getByLabel('State').selectOption({ label: 'All' });
	await page.getByLabel('Search requests').fill('req-1');
	await expect(page.getByTestId('request-row')).toHaveCount(1);
	await expect(page.getByTestId('request-row')).toContainText('REQ-1');
});

test('RQS-4: choosing Someone else without choosing who does not settle a handle nobody has', async ({ page }) => {
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	await dialog.getByLabel('What do you need?').fill('@nobody-here do the thing');
	await dialog.getByRole('radio', { name: /^Someone else/ }).check();
	await expect(dialog.getByTestId('no-such-handle')).toBeVisible();
	await expect(dialog.locator('button[type=submit]')).toBeDisabled();
	await dialog.locator('select[aria-label="Someone else"]').selectOption({ index: 1 });
	await expect(dialog.getByTestId('no-such-handle')).toHaveCount(0);
	await expect(dialog.locator('button[type=submit]')).toBeEnabled();
});

test('RQS-4: completing a handle keeps the caret at the handle, and every dash or punctuation after it is dropped from the saved words', async ({ page }) => {
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	const words = dialog.getByLabel('What do you need?');
	await words.fill('@sa fix the build');
	await words.evaluate((el: HTMLTextAreaElement) => el.setSelectionRange(3, 3));
	await words.dispatchEvent('keyup');
	await words.press('Enter');
	const handle = (await words.inputValue()).split(' ')[0];
	expect(await words.evaluate((el: HTMLTextAreaElement) => el.selectionStart)).toBe(handle.length);
	await words.fill('@sam,, - fix the build');
	await dialog.getByRole('button', { name: 'Send to Sam Okafor' }).click();
	await expect(page.getByRole('button', { name: 'fix the build', exact: true })).toBeVisible();
});

test('RQS-15: choosing the Approved state shows its rows, not a collapsed group', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Costs per project');
	await agentScopes(page, 'Costs per project');
	await visit(page, '/work');
	await page.getByRole('tab', { name: /^Requests/ }).click();
	await page.getByRole('button', { name: /^Review REQ-1/ }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByRole('button', { name: /^Approve and create 2 tasks/ }).click();
	await sheet.getByLabel('Which project?').selectOption({ label: 'Atlas' });
	await sheet.getByRole('button', { name: 'Create 2 tasks' }).click();
	await page.keyboard.press('Escape');
	await page.getByLabel('State').selectOption({ label: 'Approved' });
	await expect(page.getByTestId('request-row')).toBeVisible(); // visible, not only counted
});

test('RQS-13: what was found stays while the words change', async ({ page }) => {
	await open(page);
	await queue(page);
	await capture(page, 'Dark mode for the docs reader', { button: /^Just save$/ });
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	const words = dialog.getByLabel('What do you need?');
	await words.fill('I would like a dark mode when reading the docs');
	await expect(dialog.getByTestId('similar')).toBeVisible();
	await words.pressSequentially(' please');
	await expect(dialog.getByTestId('similar')).toBeVisible(); // no flicker while the next answer is awaited
});

test('F1: the capture panel fits a small window: it scrolls inside and Send stays in view', async ({ page }) => {
	await page.setViewportSize({ width: 390, height: 520 });
	await open(page);
	await queue(page);
	await page.getByTestId('new-request').click();
	const dialog = page.getByRole('dialog', { name: 'New request' });
	await expect(dialog.getByLabel('What do you need?')).toBeVisible();
	const box = await dialog.boundingBox();
	expect(box!.y).toBeGreaterThanOrEqual(0);
	expect(box!.y + box!.height).toBeLessThanOrEqual(520);
	expect(box!.x).toBeGreaterThanOrEqual(0);
	expect(box!.x + box!.width).toBeLessThanOrEqual(390);
	await expect(dialog.locator('button[type=submit]')).toBeInViewport();
});

test('F12: Work reopens on the tab it was left on', async ({ page }) => {
	await open(page);
	await queue(page);
	await page.reload();
	await page.locator('html[data-hydrated="true"]').waitFor();
	await expect(page.getByRole('tab', { name: /^Requests/ })).toHaveAttribute('aria-selected', 'true');
	await page.getByRole('tab', { name: 'Tasks' }).click();
	await page.reload();
	await page.locator('html[data-hydrated="true"]').waitFor();
	await expect(page.getByRole('tab', { name: 'Tasks' })).toHaveAttribute('aria-selected', 'true');
});
