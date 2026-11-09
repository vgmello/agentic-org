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

test('RQS-2/3/5: the words are enough, the route is suggested by rule with a reason and can be changed, and it lands With agents', async ({ page }) => {
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

test('RQS-9/14: scoped by an agent it needs you; Approve asks for a project, lets the drafts be edited, and makes the tasks', async ({ page }) => {
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
	// The tasks are in the project.
	await visit(page, '/work');
	await expect(page.getByTestId('work-row')).toHaveCount(2);
	await expect(page.getByTestId('work-project')).toHaveText(['Atlas', 'Atlas']);
});

test('RQS-10: asking for changes sends it back to the agent with the note; declining needs a reason; both are in the history', async ({ page }) => {
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
	// The reason of the pick is kept, and it is not called the rule's.
	await expect(sheet.getByTestId('route-reason')).toContainText('Sent because:');
	await expect(sheet.getByTestId('route-reason')).not.toContainText('suggested by rule');
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
	await expect(page.getByRole('dialog').getByTestId('route-reason')).toContainText('Sent because:');
	await expect(page.getByRole('dialog').getByTestId('route-reason')).not.toContainText('suggested by rule');
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
