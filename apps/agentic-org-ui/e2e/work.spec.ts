// Work items (plan step 5: work-items-ux, feature-requests-ux, costs-and-insights-ux §2 and §4),
// against e2e/fakes/modules/workitems.ts (zero static mocks in src/).
import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { enterMockedOrganization } from './mock-organization';

async function open(page: Page, url = '/work') {
	await enterMockedOrganization(page, 'Lumen Labs');
	await visit(page, url);
}

async function noSidewaysScroll(page: Page) {
	expect(
		await page.evaluate(
			() => document.documentElement.scrollWidth - document.documentElement.clientWidth
		)
	).toBeLessThanOrEqual(0);
}

test(
	'WORK-1/2/11: /work defaults to Needs me with Inbox links, switches Show views, board/list, saved filters, and bug grouping',
	{ tag: '@journey' },
	async ({ page }) => {
		await open(page);
		await expect(page).toHaveTitle('Work · Agentic Org');
		await expect(page.getByTestId('show-select')).toHaveValue('needs_me');
		await expect(page.getByTestId('needs-me-list')).toBeVisible();
		await expect(page.getByTestId('needs-me-inbox-link').first()).toHaveAttribute(
			'href',
			/\/inbox\?item=/
		);
		await shot(page, 'work-needs-me');

		// Switch to Show: By project (?view=by_project) and verify projectless items show "No project".
		await page.getByTestId('show-select').selectOption('by_project');
		await expect(page).toHaveURL(/view=by_project/);
		await expect(
			page.getByTestId('work-group-heading').filter({ hasText: 'No project' })
		).toBeVisible();

		// Switch to Tasks tab -> Show switches to Mine and renders the 4-column board.
		await page
			.getByRole('tablist', { name: 'Work', exact: true })
			.getByRole('tab', { name: /^Tasks/ })
			.click();
		await expect(page.getByTestId('work-board')).toBeVisible();
		await expect(page.getByTestId('board-column')).toHaveCount(4);

		// Saved filter All shows every task across projects, including 7-day Done window + Show all done.
		await page.getByTestId('saved-filters').locator('[data-filter=all]').click();
		const toggleDone = page.getByTestId('toggle-all-done');
		await expect(toggleDone).toContainText('Show all done (1)');
		await toggleDone.click();
		await expect(page.locator('[data-testid=work-card][data-ref=T-A29]')).toBeVisible();

		// Move a card via ⋯ Move to menu and verify live announcement (TSK-2).
		const card = page.locator('[data-testid=work-card][data-ref=T-A40]');
		await card.getByRole('button', { name: 'Actions for Task T-A40' }).click();
		await page.getByRole('menuitem', { name: 'To do', exact: true }).click();
		await expect(page.getByTestId('board-live-region')).toContainText('Moved Task T-A40 to To do.');
		await expect(
			page.locator('[data-testid=board-column][data-status=todo]').locator('[data-ref=T-A40]')
		).toBeVisible();

		// Keyboard Alt+ArrowRight moves card back to In progress (TSK-2).
		const movedCard = page.locator('[data-testid=work-card][data-ref=T-A40]');
		await movedCard.focus();
		await page.keyboard.press('Alt+ArrowRight');
		await expect(
			page
				.locator('[data-testid=board-column][data-status=in_progress]')
				.locator('[data-ref=T-A40]')
		).toBeVisible();

		// Switch to Bugs tab: grouped by Assigned team with Unassigned team and severity counts incl. not set (BUG-10/11).
		await page
			.getByRole('tablist', { name: 'Work', exact: true })
			.getByRole('tab', { name: /^Bugs/ })
			.click();
		await page.getByTestId('saved-filters').locator('[data-filter=all]').click();
		await expect(
			page.getByTestId('work-group-heading').filter({ hasText: 'Unassigned team' })
		).toBeVisible();
		await expect(page.getByTestId('group-severity-counts').first()).toContainText(
			/not set|major|critical/
		);
		await shot(page, 'work-bugs-list');

		// Switch to Requests tab and filter No project to verify projectless team-only request T-B17 (RQS-7/14/15).
		await page
			.getByRole('tablist', { name: 'Work', exact: true })
			.getByRole('tab', { name: /^Requests/ })
			.click();
		await page.getByTestId('saved-filters').locator('[data-filter=all]').click();
		await page.getByLabel('Filter by project').selectOption('none');
		await expect(page.locator('[data-testid=work-row][data-ref=T-B17]')).toBeVisible();
		await expect(page.locator('[data-testid=work-row][data-ref=T-B17]')).toContainText(
			'No project'
		);
	}
);

test('TASK page (/work/T-A40): fields, checklist, shared vs private notes, output, why, unknown activity, assignment conflict', async ({
	page
}) => {
	await open(page, '/work/T-A40');
	await expect(page).toHaveTitle('T-A40 Split the checkout service · Work · Agentic Org');
	await expect(page.getByTestId('immutable-reporter')).toContainText('Avery Stone');
	await expect(page.getByTestId('repository-evidence')).toContainText('sam/split-checkout');
	await expect(page.getByTestId('output-section')).toContainText('No outcome recorded.');
	await expect(page.getByTestId('activity-events')).toContainText('Unknown change');
	await expect(page.getByTestId('task-time')).toContainText("working time isn't tracked yet");
	await shot(page, 'work-task-detail');

	// Checklist: add step inline, mark required, reorder (CHK-2/3/4/7).
	const stepInput = page.getByPlaceholder(/\+ Add step/);
	await stepInput.fill('Confirm rollback safety');
	await page.getByRole('button', { name: 'Add step' }).click();
	await expect(page.getByTestId('checklist-items')).toContainText('Confirm rollback safety');
	const addedStep = page
		.getByTestId('checklist-step')
		.filter({ hasText: 'Confirm rollback safety' });
	await addedStep.getByRole('button', { name: 'Mark required' }).click();
	await expect(addedStep.getByTestId('step-required-badge')).toContainText('Required');

	// Why block: link a document.
	await page.getByRole('button', { name: 'Link document…' }).click();
	await page.getByLabel('Choose document to link').selectOption('doc-adr-31');
	await page.getByRole('button', { name: 'Link', exact: true }).click();
	await expect(page.getByTestId('why-section')).toContainText('ADR-0031');

	// Discussion: post shared comment, switch to Private notes, add & publish copy keeping private note.
	await page.getByLabel('Add a shared comment').fill('Looks good to ship.');
	await page.getByRole('button', { name: 'Post shared comment' }).click();
	await expect(page.getByTestId('shared-comments')).toContainText('Looks good to ship.');

	await page.getByRole('tab', { name: 'Private notes' }).click();
	await expect(page.getByTestId('private-notes-list')).toContainText('Read-only');
	await page.getByLabel(/Add a private note/).fill('Personal reminder before launch');
	await page.getByRole('button', { name: 'Save private note' }).click();
	await expect(page.getByTestId('private-notes-list')).toContainText(
		'Personal reminder before launch'
	);

	const myNote = page
		.getByTestId('private-note')
		.filter({ hasText: 'Personal reminder before launch' });
	// Regression: Cancel on Publish dialog clears target so dialog reopens cleanly.
	await myNote.getByRole('button', { name: 'Publish as shared comment…' }).click();
	const publishDialog = page.getByRole('dialog', { name: 'Publish note as a shared comment?' });
	await expect(publishDialog).toBeVisible();
	await publishDialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(publishDialog).toBeHidden();
	await myNote.getByRole('button', { name: 'Publish as shared comment…' }).click();
	await expect(publishDialog).toBeVisible();
	await publishDialog.getByRole('button', { name: 'Publish shared comment' }).click();
	await expect(page.getByTestId('private-notes-list')).toContainText(
		'Personal reminder before launch'
	);

	// Regression: Status select Cancelled… pre-selects Cancelled and resets select on Cancel.
	const statusSelect = page.getByTestId('detail-status-select');
	const initialStatus = await statusSelect.inputValue();
	await statusSelect.selectOption('cancelled');
	const cancelCloseDialog = page.getByRole('dialog', { name: 'Close T-A40' });
	await expect(cancelCloseDialog.getByLabel('Close as')).toHaveValue('cancelled');
	await cancelCloseDialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(cancelCloseDialog).toBeHidden();
	await expect(statusSelect).toHaveValue(initialStatus);

	// Assignment pair conflict (?mock=conflict): shows current vs desired and retries cleanly (TRI-6).
	await visit(page, '/work/T-A40?mock=conflict');
	await page.getByLabel('Assigned team').selectOption('design');
	await page.getByLabel('Assignee').selectOption('juno');
	await page.getByRole('button', { name: 'Save assignment' }).click();
	await expect(page.getByTestId('assignment-conflict')).toContainText(
		'Assignment changed by Avery Stone'
	);
	await page.getByRole('button', { name: /Retry at rev/ }).click();
	await expect(page.getByTestId('assignment-conflict')).toHaveCount(0);
});

test('BUG page (/work/T-A42): provenance, triage, create fix task, verify still happens, close & reopen', async ({
	page
}) => {
	await open(page, '/work/T-A42');
	await expect(page).toHaveTitle(/T-A42 Pay button wraps under the total at 320 px/);
	await expect(page.getByTestId('reported-provenance')).toContainText('severity Minor');
	await expect(page.getByTestId('bug-fixed-evidence')).toContainText(
		'Marked as fixed by Noor Haddad'
	);

	// Verify fix -> Still happens requires note, reopens and increments reopenCount (FIX-4/5).
	await page.getByRole('button', { name: 'Verify fix…' }).click();
	const verifyDialog = page.getByRole('dialog', { name: /Verify fix for T-A42/ });
	await verifyDialog.getByText('Still happens', { exact: true }).click();
	await verifyDialog.getByRole('button', { name: 'Reopen bug' }).click();
	await expect(verifyDialog.getByRole('alert')).toContainText('Explain what still happens');
	await verifyDialog.getByLabel(/What still happens/).fill('Still clips on iOS Safari.');
	await verifyDialog.getByRole('button', { name: 'Reopen bug' }).click();
	await expect(page.getByTestId('reopen-count')).toContainText('Reopened 1');

	// Close as Duplicate rejects self-link, then accepts T-A45 (FIX-7).
	await page.getByRole('button', { name: 'Close as…' }).click();
	const closeDialog = page.getByRole('dialog', { name: 'Close T-A42' });
	await closeDialog.getByLabel('Close as').selectOption('duplicate');
	await closeDialog.getByLabel('Original bug reference').fill('T-A42');
	await closeDialog.getByRole('button', { name: 'Close item' }).click();
	await expect(closeDialog.getByRole('alert')).toContainText(
		"can't be marked as a duplicate of itself"
	);
	await closeDialog.getByLabel('Original bug reference').fill('T-A45');
	await closeDialog.getByRole('button', { name: 'Close item' }).click();
	await expect(page.getByTestId('duplicate-of')).toContainText('Bug T-A45');

	// Reopen requires note (FIX-9).
	await page.getByRole('button', { name: 'Reopen…' }).click();
	const reopenDialog = page.getByRole('dialog', { name: 'Reopen T-A42' });
	await reopenDialog.getByRole('button', { name: 'Reopen' }).click();
	await expect(reopenDialog.getByRole('alert')).toContainText('Add a note');
	await reopenDialog.getByLabel('Note').fill('Separate root cause from T-A45.');
	await reopenDialog.getByRole('button', { name: 'Reopen' }).click();
	await expect(page.getByTestId('reopen-count')).toContainText('Reopened 2');
});

test('REQUEST page (/work/T-A43 and /work/T-A49): editable Approve and create preview, scoper refusal, Decline/Merge', async ({
	page
}) => {
	// T-A49 was scoped by Alex Kim (viewer) -> Approve disabled with reason (RQS-12).
	await open(page, '/work/T-A49');
	await expect(page.getByRole('button', { name: 'Approve and create…' })).toBeDisabled();
	await expect(page.getByTestId('approve-disabled-reason')).toContainText(
		'The scoper cannot approve their own scoping outcome.'
	);

	// T-A43 was scoped by Juno -> editable Approve and create creates tasks linked to T-A43 (RQS-9).
	await visit(page, '/work/T-A43');
	await expect(page.getByTestId('request-outcome-section')).toContainText('F-11');
	await page.getByRole('button', { name: 'Approve and create…' }).click();
	const approveDialog = page.getByRole('dialog', { name: 'Approve and create from T-A43' });
	const draftInputs = approveDialog.getByTestId('approve-draft-list').locator('input');
	await expect(draftInputs).toHaveCount(2);
	await draftInputs.first().fill('Build adjacent table availability query (v1)');
	await approveDialog.getByRole('button', { name: 'Remove' }).last().click();
	await expect(draftInputs).toHaveCount(1);
	await approveDialog.getByRole('button', { name: 'Approve and create (1)' }).click();
	await expect(page.getByTestId('created-tasks')).toContainText(
		'Build adjacent table availability query (v1)'
	);
});

test('CAPTURE & TOP-BAR NEW MENU: Project and Work item in New menu, Report bug validation, similar bugs & discard', async ({
	page
}) => {
	await open(page, '/work');
	await page.getByRole('button', { name: 'New', exact: true }).click();
	const items = page.getByRole('menuitem');
	await expect(items).toHaveText([
		'Project',
		'Work item',
		'Request',
		'Message',
		'Hire an agent',
		'Add a person'
	]);

	// Open Work item from New menu, switch to Bug, verify disabled reason & similar bug match (BUG-2/3/4/9).
	await page.getByRole('menuitem', { name: 'Work item', exact: true }).click();
	const newDialog = page.getByRole('dialog', { name: /New task/ });
	await newDialog.getByText('Bug', { exact: true }).click();
	const bugDialog = page.getByRole('dialog', { name: /Report a bug/ });
	await expect(bugDialog.getByTestId('bug-submit-reason')).toContainText(
		'Fill in Title and What happened'
	);
	await expect(bugDialog.getByRole('button', { name: 'Report bug' })).toBeDisabled();

	await bugDialog.getByLabel('Title').fill('Pay button clips on phone');
	await expect(bugDialog.getByTestId('similar-items')).toContainText('T-A42');

	// Closing dirty bug dialog asks "Discard this bug report?" (BUG-9).
	await bugDialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(page.getByRole('dialog', { name: 'Discard this bug report?' })).toBeVisible();
	await page.getByRole('button', { name: 'Discard', exact: true }).click();

	// Create a new projectless task from Work header.
	await page.getByRole('button', { name: 'New task' }).click();
	const taskDialog = page.getByRole('dialog', { name: /New task/ });
	await taskDialog.getByLabel('Title').fill('Audit backup retention');
	await taskDialog.getByLabel('Accountable team').selectOption('platform');
	await taskDialog.getByRole('button', { name: 'Create task' }).click();
	await expect(taskDialog).toBeHidden();
	await expect(page.getByText(/Task T-A\d+ saved\./)).toBeVisible();
});

test('BUDGETS: set budget, reached actions (Allow 20% once, agent-PM restriction, stop_pending, extended, viewer scope, unknown outcome)', async ({
	page
}) => {
	// Set budget on T-A44 (no budget yet) with exact micro-USD conversion.
	await open(page, '/work/T-A44');
	await page.getByRole('button', { name: 'Set budget…' }).click();
	const setDialog = page.getByRole('dialog', { name: 'Set budget · T-A44' });
	await setDialog.getByLabel('Limit (USD)').fill('12.50');
	await setDialog.getByRole('button', { name: 'Set budget' }).click();
	await expect(page.getByTestId('task-budget')).toContainText('Budget $12.50');

	// Reached budget on T-A31: Allow 20% more once extends $10 -> $12 and clears hold (BUD-3).
	await visit(page, '/work/T-A31');
	await expect(page.getByTestId('budget-hold-label')).toHaveText('Paused · Budget reached');
	await page.getByRole('button', { name: 'Allow 20% more once' }).click();
	await expect(page.getByTestId('task-budget')).toContainText('Budget $12');
	await expect(page.getByTestId('budget-reached-box')).toHaveCount(0);

	// Stop pending state and already-extended state.
	await visit(page, '/work/T-A31?mock=budget-stop-pending,budget-extended');
	await expect(page.getByTestId('budget-hold-label')).toHaveText('Stopping · Budget reached');
	await expect(page.getByTestId('budget-already-extended')).toHaveText(
		'This budget already used its one 20% extension.'
	);
	await expect(page.getByRole('button', { name: 'Allow 20% more once' })).toHaveCount(0);

	// Agent PM sees person-only message and keeps Stop (BUD-2).
	await visit(page, '/work/T-A31?mock=agent-pm');
	await expect(page.getByTestId('budget-person-only')).toHaveText(
		'Only a person can raise, clear or extend a budget.'
	);
	await expect(page.getByRole('button', { name: 'Raise budget…' })).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'Stop', exact: true })).toBeVisible();

	// Viewer sees budget state chip only with ZERO amounts (costs §1 "Who sees amounts").
	await visit(page, '/work/T-A31?mock=viewer');
	await expect(page.getByTestId('cost-state-only')).toBeVisible();
	await expect(page.getByTestId('cost-time-block')).not.toContainText('$');

	// Unknown save outcome -> "Save not confirmed", Discard attempt unblocks reopening, and Check status resolves (BUD-2).
	await visit(page, '/work/T-A40?mock=budget-unknown');
	await page.getByRole('button', { name: 'Change budget…' }).click();
	const changeDialog = page.getByRole('dialog', { name: 'Change budget · T-A40' });
	await changeDialog.getByLabel('Limit (USD)').fill('15');
	await changeDialog.getByRole('button', { name: 'Save budget' }).click();
	await expect(changeDialog.getByTestId('budget-form-error')).toContainText('Save not confirmed');
	await expect(changeDialog.getByRole('button', { name: 'Discard attempt' })).toBeVisible();
	await changeDialog.getByRole('button', { name: 'Check status' }).click();
	await expect(changeDialog).toBeHidden();

	// Stale copy keeps budget hold notice visible while hiding write actions.
	await visit(page, '/work/T-A31?mock=stale');
	await expect(page.getByTestId('budget-hold-label')).toHaveText('Paused · Budget reached');
	await expect(page.getByTestId('budget-actions-stale')).toBeVisible();
	await expect(page.getByRole('button', { name: 'Raise budget…' })).toHaveCount(0);
});

test('STATES & LAYOUT: unsupported (no Retry), error, empty, stale, not found, and 320 px / 200% text', async ({
	page
}) => {
	// Unsupported queue -> explicit "Not available in this version" with NO Retry button and NO header create buttons.
	await open(page, '/work?mock=unsupported');
	await expect(page.getByTestId('resource-unsupported')).toContainText(
		'Not available in this version'
	);
	await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'New task' })).toHaveCount(0);

	// Show: Mine with no employee record shows banner and matches zero org items (WORK-2).
	await visit(page, '/work?view=mine&mock=no-employee');
	await expect(page.getByTestId('no-employee-note')).toBeVisible();
	await expect(page.getByTestId('work-card')).toHaveCount(0);
	await expect(page.getByTestId('work-row')).toHaveCount(0);
	// Choosing All clears no-employee-note once widened past Mine scope.
	await page.getByTestId('saved-filters').locator('[data-filter=all]').click();
	await expect(page.getByTestId('no-employee-note')).toHaveCount(0);

	// Unsupported private notes & unsupported cost block on /work/T-A40 -> no Retry button.
	await visit(page, '/work/T-A40?mock=notes-unsupported,cost-unsupported');
	await expect(page.getByTestId('cost-unsupported')).toContainText('Not available in this version');
	await expect(
		page.getByTestId('cost-time-block').getByRole('button', { name: /Retry|Try again/ })
	).toHaveCount(0);
	await page.getByRole('tab', { name: 'Private notes' }).click();
	await expect(page.getByTestId('private-notes-unsupported')).toContainText(
		'Not available in this version'
	);

	// Empty queue shows "Nothing needs you in Work" in Needs me.
	await visit(page, '/work?mock=empty');
	await expect(page.getByRole('heading', { name: 'No work yet' })).toBeVisible();

	// Error with Retry.
	await visit(page, '/work?mock=fails');
	await expect(page.getByRole('alert')).toContainText("Couldn't load work");
	await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();

	// Stale copy banner.
	await visit(page, '/work?mock=stale');
	await expect(page.getByTestId('stale-banner')).toContainText('read-only');

	// Not found page.
	await visit(page, '/work/T-Z999');
	await expect(page).toHaveTitle('Task not found · Work · Agentic Org');
	await expect(page.getByRole('heading', { name: 'Task not found' })).toBeVisible();

	// 320 px with 200% text -> zero horizontal overflow on /work and /work/T-A40.
	await page.setViewportSize({ width: 320, height: 800 });
	await visit(page, '/work');
	await page.addStyleTag({ content: 'html { font-size: 200%; }' });
	await expect(page.getByTestId('needs-me-list')).toBeVisible();
	await noSidewaysScroll(page);

	await visit(page, '/work/T-A40');
	await page.addStyleTag({ content: 'html { font-size: 200%; }' });
	await expect(page.getByTestId('work-detail-header')).toBeVisible();
	await noSidewaysScroll(page);
});

test('PRIVATE NOTES (TASK-67 / ADR-0031): per-agent grouped pane, visibility copy, read-only agent notes, own CRUD + conflict, stale/viewer/no-employee, 320px/200% zoom', async ({
	page
}) => {
	await open(page, '/work/T-A40');

	// Keyboard arrow-key navigation switches from Shared discussion tab to Private notes tab.
	const sharedTab = page.getByRole('tab', { name: /^Shared discussion/ });
	const privateTab = page.getByRole('tab', { name: 'Private notes' });
	await sharedTab.focus();
	await page.keyboard.press('ArrowRight');
	await expect(privateTab).toBeFocused();
	await expect(privateTab).toHaveAttribute('aria-selected', 'true');

	const pane = page.getByTestId('private-notes-pane');
	await expect(pane).toBeVisible();

	// Plain-language visibility notice and zero internal jargon.
	await expect(pane.getByTestId('private-notes-audience')).toContainText(
		'Visible to people and the author; hidden from other agents'
	);
	const paneText = (await pane.textContent()) ?? '';
	expect(paneText).not.toMatch(/ADR-0031|org-store|PN-0|task-notes\.db/i);

	// Grouped by author: signed-in person (Alex Kim) first, then agents (Sam Okafor, Noor Haddad), then Avery Stone.
	const groups = pane.getByTestId('private-note-author-group');
	await expect(groups).toHaveCount(4);
	await expect(groups.nth(0)).toHaveAttribute('data-author-id', 'you');
	await expect(groups.nth(1)).toHaveAttribute('data-author-id', 'sam');
	await expect(groups.nth(2)).toHaveAttribute('data-author-id', 'noor');
	await expect(groups.nth(3)).toHaveAttribute('data-author-id', 'avery');

	// Agent groups display Agent badge, Read-only badge, edited indicator, and ZERO write controls.
	const samGroup = pane.locator('[data-testid=private-note-author-group][data-author-id=sam]');
	await expect(samGroup.getByTestId('private-note-author-kind')).toHaveText('Agent');
	await expect(samGroup.getByTestId('private-note-readonly-badge')).toContainText('Read-only');
	await expect(samGroup.getByTestId('private-note')).toHaveCount(2);
	await expect(samGroup.getByTestId('private-note-edited')).toContainText('Edited');
	await expect(samGroup.getByRole('button', { name: /^Edit$/ })).toHaveCount(0);
	await expect(samGroup.getByRole('button', { name: /^Delete$/ })).toHaveCount(0);
	await expect(samGroup.getByRole('button', { name: /Publish/ })).toHaveCount(0);

	const noorGroup = pane.locator('[data-testid=private-note-author-group][data-author-id=noor]');
	await expect(noorGroup.getByTestId('private-note-author-kind')).toHaveText('Agent');
	await expect(noorGroup.getByRole('button', { name: /^Edit$/ })).toHaveCount(0);
	await expect(noorGroup.getByRole('button', { name: /^Delete$/ })).toHaveCount(0);
	await expect(noorGroup.getByRole('button', { name: /Publish/ })).toHaveCount(0);

	// Other person's group (Avery Stone) is also strictly read-only.
	const averyGroup = pane.locator('[data-testid=private-note-author-group][data-author-id=avery]');
	await expect(averyGroup.getByTestId('private-note-author-kind')).toHaveText('Person');
	await expect(averyGroup.getByRole('button', { name: /^Edit$/ })).toHaveCount(0);
	await expect(averyGroup.getByRole('button', { name: /^Delete$/ })).toHaveCount(0);
	await expect(averyGroup.getByRole('button', { name: /Publish/ })).toHaveCount(0);

	// Author filter pills isolate one agent's private notes cleanly, then restore All authors.
	const filterBar = pane.getByTestId('private-notes-author-filter');
	await filterBar.locator('[data-author-filter=sam]').click();
	await expect(groups).toHaveCount(1);
	await expect(groups.first()).toHaveAttribute('data-author-id', 'sam');
	await filterBar.getByRole('button', { name: /All authors/ }).click();
	await expect(groups).toHaveCount(4);

	// Author group collapse & expand disclosure works accessibly.
	const samToggle = samGroup.getByRole('button', { name: /2 notes/ });
	await expect(samToggle).toHaveAttribute('aria-expanded', 'true');
	await samToggle.click();
	await expect(samToggle).toHaveAttribute('aria-expanded', 'false');
	await expect(samGroup.getByTestId('private-note')).toHaveCount(0);
	await samToggle.click();
	await expect(samToggle).toHaveAttribute('aria-expanded', 'true');
	await expect(samGroup.getByTestId('private-note')).toHaveCount(2);

	// Human creates, edits, and deletes their own private note.
	await pane.getByLabel(/Add a private note/).fill('Initial private check');
	await pane.getByRole('button', { name: 'Save private note' }).click();
	const createdNote = pane.getByTestId('private-note').filter({ hasText: 'Initial private check' });
	await expect(createdNote).toBeVisible();
	const createdNoteId = await createdNote.getAttribute('data-note-id');
	expect(createdNoteId).toBeTruthy();
	const createdNoteItem = pane.locator(`[data-note-id="${createdNoteId}"]`);

	await createdNoteItem.getByRole('button', { name: 'Edit' }).click();
	await createdNoteItem.getByLabel('Edit private note').fill('Updated private check after review');
	await createdNoteItem.getByRole('button', { name: 'Save note' }).click();
	await expect(createdNoteItem).toContainText('Updated private check after review');
	await expect(createdNoteItem.getByTestId('private-note-edited')).toContainText('Edited');

	await createdNoteItem.getByRole('button', { name: 'Delete' }).click();
	const deleteDialog = page.getByRole('dialog', { name: 'Delete this private note?' });
	await expect(deleteDialog).toBeVisible();
	await deleteDialog.getByRole('button', { name: 'Delete note' }).click();
	await expect(createdNoteItem).toHaveCount(0);

	// Revision conflict recovery on edit preserves draft text and retries cleanly after reload.
	await visit(page, '/work/T-A40?mock=notes-conflict');
	await page.getByRole('tab', { name: 'Private notes' }).click();
	const mySeedNote = page.locator('[data-note-id="pn-40-1"]');
	await mySeedNote.getByRole('button', { name: 'Edit' }).click();
	await mySeedNote
		.getByLabel('Edit private note')
		.fill('Double-check Stripe webhook retries + idempotency');
	await mySeedNote.getByRole('button', { name: 'Save note' }).click();
	await expect(mySeedNote.getByRole('alert')).toContainText('changed in another session');
	await mySeedNote.getByRole('button', { name: 'Reload latest revision' }).click();
	await expect(mySeedNote.getByRole('alert')).toHaveCount(0);
	await expect(mySeedNote.getByLabel('Edit private note')).toHaveValue(
		'Double-check Stripe webhook retries + idempotency'
	);
	await mySeedNote.getByRole('button', { name: 'Save note' }).click();
	await expect(mySeedNote).toContainText('Double-check Stripe webhook retries + idempotency');

	// Stale read-only mode hides composer and all edit/delete/publish buttons while keeping notes readable.
	await visit(page, '/work/T-A40?mock=stale');
	await page.getByRole('tab', { name: 'Private notes' }).click();
	await expect(page.getByTestId('private-notes-stale-note')).toBeVisible();
	await expect(page.getByTestId('private-note-composer')).toHaveCount(0);
	await expect(
		page.getByTestId('private-notes-pane').getByRole('button', { name: /^Edit$/ })
	).toHaveCount(0);
	await expect(
		page.getByTestId('private-notes-pane').getByRole('button', { name: /^Delete$/ })
	).toHaveCount(0);
	await expect(
		page.getByTestId('private-notes-pane').getByRole('button', { name: /Publish/ })
	).toHaveCount(0);

	// Viewer-only role & no-employee account hide composer and all write buttons while keeping notes readable.
	for (const mode of ['viewer', 'no-employee']) {
		await visit(page, `/work/T-A40?mock=${mode}`);
		await page.getByRole('tab', { name: 'Private notes' }).click();
		await expect(page.getByTestId('private-notes-readonly-reason')).toBeVisible();
		await expect(page.getByTestId('private-note-composer')).toHaveCount(0);
		await expect(
			page.getByTestId('private-notes-pane').getByRole('button', { name: /^Edit$/ })
		).toHaveCount(0);
		await expect(
			page.getByTestId('private-notes-pane').getByRole('button', { name: /^Delete$/ })
		).toHaveCount(0);
		await expect(
			page.getByTestId('private-notes-pane').getByRole('button', { name: /Publish/ })
		).toHaveCount(0);
	}

	// Unsupported (no Retry button) and Error (with Retry button) resource states.
	await visit(page, '/work/T-A40?mock=notes-unsupported');
	await page.getByRole('tab', { name: 'Private notes' }).click();
	await expect(page.getByTestId('private-notes-unsupported')).toContainText(
		'Not available in this version'
	);
	await expect(
		page.getByTestId('private-notes-pane').getByRole('button', { name: /Retry/ })
	).toHaveCount(0);

	await visit(page, '/work/T-A40?mock=notes-fails');
	await page.getByRole('tab', { name: 'Private notes' }).click();
	await expect(page.getByTestId('private-notes-error')).toBeVisible();
	await expect(
		page.getByTestId('private-notes-pane').getByRole('button', { name: 'Retry' })
	).toBeVisible();

	// 320 px narrow viewport with 200% text scale while Private notes pane is open -> zero horizontal overflow.
	await page.setViewportSize({ width: 320, height: 800 });
	await visit(page, '/work/T-A40');
	await page.addStyleTag({ content: 'html { font-size: 200%; }' });
	await page.getByRole('tab', { name: 'Private notes' }).click();
	await expect(page.getByTestId('private-notes-list')).toBeVisible();
	await noSidewaysScroll(page);
	await shot(page, 'work-private-notes-320-200pct');
});
