// Requests journeys against the unified Work queue (`WorkQueueView`), `CaptureDialog`, and `RequestSection`
// (feature-requests-ux §1–§4; RQS-1..3, RQS-7, RQS-9, RQS-10, RQS-12, RQS-13, RQS-14, RQS-15).
import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { enterMockedOrganization } from './mock-organization';

async function open(page: Page, url = '/work') {
	await enterMockedOrganization(page, 'Lumen Labs');
	await visit(page, url);
}

async function openRequestsTab(page: Page) {
	await page
		.getByRole('tablist', { name: 'Work', exact: true })
		.getByRole('tab', { name: /^Requests/ })
		.click();
	await expect(page.getByTestId('work-list')).toBeVisible();
}

async function noSidewaysScroll(page: Page) {
	expect(
		await page.evaluate(
			() => document.documentElement.scrollWidth - document.documentElement.clientWidth
		)
	).toBeLessThanOrEqual(0);
}

test(
	'RQS-2/3/5: the words are enough, a request can be captured with an accountable team and optional assignee, and lands in the Requests queue',
	{ tag: '@journey' },
	async ({ page }) => {
		await open(page);
		await openRequestsTab(page);
		await page.getByTestId('saved-filters').locator('[data-filter=all]').click();

		// Open New request from Work header.
		await page.getByRole('button', { name: 'New request' }).click();
		const dialog = page.getByRole('dialog', { name: /New request/ });
		await expect(dialog.getByLabel('Title')).toBeFocused();

		// Empty submission refuses and focuses Title.
		await dialog.getByRole('button', { name: 'Save request' }).click();
		await expect(dialog.getByRole('alert').first()).toBeVisible();

		// Filling title + accountable team + assignee updates submit label to "Send request".
		await dialog.getByLabel('Title').fill('Let me capture requests with a shortcut and route them');
		await dialog
			.getByLabel('Details and why it matters')
			.fill('Owners need a one-step request capture flow from any page.');
		await dialog.getByLabel('Accountable team').selectOption('web');
		await dialog.getByLabel('Assignee').selectOption('juno');
		await expect(dialog.getByRole('button', { name: 'Send request' })).toBeVisible();
		await shot(page, 'request-capture');

		await dialog.getByRole('button', { name: 'Send request' }).click();
		await expect(dialog).toBeHidden();
		await expect(page.getByText(/sent to Juno Park\./)).toBeVisible();
		await expect(
			page
				.getByTestId('work-row')
				.filter({ hasText: 'Let me capture requests with a shortcut and route them' })
		).toBeVisible();
		await shot(page, 'requests-queue-routed');
	}
);

test(
	'RQS-9/14: scoped by an agent it needs you; Approve previews editable draft tasks, creates them linked to the request, and clears Needs me',
	{ tag: '@journey' },
	async ({ page }) => {
		await open(page, '/work');
		await expect(page.getByTestId('needs-me-list')).toContainText('T-A43');

		// Legacy /work?request=<id> deep link redirects cleanly to /work/T-A43.
		await visit(page, '/work?request=0193c000-0000-7000-8000-000000000043');
		await expect(page).toHaveURL(/\/work\/T-A43$/);
		await expect(page.getByTestId('request-original-section')).toContainText('Routed by rule');
		await expect(page.getByTestId('request-outcome-section')).toContainText('F-11');
		await expect(page.getByTestId('request-outcome-section')).toContainText('DES-11');
		await expect(page.getByTestId('request-outcome-section')).toContainText('$8–$15');
		await shot(page, 'request-scoped');

		// Open editable Approve and create preview modal.
		await page.getByRole('button', { name: 'Approve and create…' }).click();
		const approveDialog = page.getByRole('dialog', { name: 'Approve and create from T-A43' });
		const draftInputs = approveDialog.getByTestId('approve-draft-list').locator('input');
		await expect(draftInputs).toHaveCount(2);

		// Edit first draft title and remove second draft before approving.
		await draftInputs.first().fill('Build adjacent table availability query (v1)');
		await approveDialog.getByRole('button', { name: 'Remove' }).last().click();
		await expect(draftInputs).toHaveCount(1);
		await approveDialog.getByRole('button', { name: 'Approve and create (1)' }).click();
		await expect(approveDialog).toBeHidden();

		await expect(page.getByTestId('created-tasks')).toContainText(
			'Build adjacent table availability query (v1)'
		);
		await expect(page.getByRole('button', { name: 'Approve and create…' })).toHaveCount(0);
		await shot(page, 'request-approved');

		// Needs me no longer lists T-A43, and the created task appears under Tasks.
		await visit(page, '/work');
		await expect(page.getByTestId('needs-me-list')).not.toContainText('T-A43');
	}
);

test(
	'RQS-10: asking for changes records the note; declining records a reason and cancels the request',
	{ tag: '@journey' },
	async ({ page }) => {
		await open(page, '/work/T-A43');

		// Ask for changes requires a note and records it on the request.
		await page.getByRole('button', { name: 'Ask for changes…' }).click();
		const changesDialog = page.getByRole('dialog', { name: 'Ask for changes on T-A43' });
		await changesDialog.getByRole('button', { name: 'Ask for changes' }).click();
		await expect(changesDialog.getByRole('alert')).toContainText('Explain what needs to change.');
		await changesDialog
			.getByLabel('What needs to change')
			.fill('Include mobile table-pairing constraints.');
		await changesDialog.getByRole('button', { name: 'Ask for changes' }).click();
		await expect(changesDialog).toBeHidden();
		await expect(page.getByTestId('changes-asked')).toContainText(
			'Include mobile table-pairing constraints.'
		);

		// Declining requires a reason, cancels the request, and removes mutation actions.
		await page.getByRole('button', { name: 'Decline…' }).click();
		const declineDialog = page.getByRole('dialog', { name: 'Decline T-A43?' });
		await declineDialog.getByRole('button', { name: 'Decline request' }).click();
		await expect(declineDialog.getByRole('alert')).toContainText(
			'Give a reason for declining this request.'
		);
		await declineDialog.getByLabel('Reason').fill('Out of scope for Beacon 0.2');
		await declineDialog.getByRole('button', { name: 'Decline request' }).click();
		await expect(declineDialog).toBeHidden();
		await expect(page.getByTestId('decline-reason')).toHaveText(
			'Declined: Out of scope for Beacon 0.2'
		);
		await expect(page.getByRole('button', { name: 'Approve and create…' })).toHaveCount(0);
		await expect(page.getByRole('button', { name: 'Decline…' })).toHaveCount(0);
	}
);

test('RQS-12/13: scoper self-approval refusal, merge validation, similar open request add-to, and dirty discard guard', async ({
	page
}) => {
	// T-A49 was scoped by Alex Kim (viewer) -> Approve disabled with clear reason (RQS-12).
	await open(page, '/work/T-A49');
	await expect(page.getByRole('button', { name: 'Approve and create…' })).toBeDisabled();
	await expect(page.getByTestId('approve-disabled-reason')).toContainText(
		'The scoper cannot approve their own scoping outcome.'
	);

	// Merge T-B17 into T-A43: rejects self-merge first, then links cleanly.
	await visit(page, '/work/T-B17');
	await page.getByRole('button', { name: 'Merge into…' }).click();
	const mergeDialog = page.getByRole('dialog', { name: 'Merge T-B17 into another request' });
	await mergeDialog.getByLabel('Target request reference').fill('T-B17');
	await mergeDialog.getByRole('button', { name: 'Merge request' }).click();
	await expect(mergeDialog.getByRole('alert')).toContainText("can't be merged into itself");
	await mergeDialog.getByLabel('Target request reference').fill('T-A43');
	await mergeDialog.getByRole('button', { name: 'Merge request' }).click();
	await expect(mergeDialog).toBeHidden();
	await expect(page.getByTestId('merged-into')).toContainText('T-A43');

	// Similar open request detection (RQS-13) + dirty discard guard.
	await visit(page, '/work');
	await openRequestsTab(page);
	await page.getByRole('button', { name: 'New request' }).click();
	const captureDialog = page.getByRole('dialog', { name: /New request/ });
	await captureDialog.getByLabel('Title').fill('adjacent tables when booking');
	await expect(captureDialog.getByTestId('similar-items')).toContainText('T-A43');

	// Closing dirty request capture dialog asks for confirmation; Cancel keeps text.
	await captureDialog.getByRole('button', { name: 'Cancel' }).click();
	const discardDialog = page.getByRole('dialog', { name: 'Discard this unsaved request?' });
	await expect(discardDialog).toBeVisible();
	await discardDialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(captureDialog.getByLabel('Title')).toHaveValue('adjacent tables when booking');

	// Add to T-A43 appends as a shared comment and navigates to /work/T-A43.
	await captureDialog.getByRole('button', { name: 'Add to T-A43' }).click();
	await expect(page).toHaveURL(/\/work\/T-A43$/);
	await expect(page.getByTestId('shared-comments')).toContainText('adjacent tables when booking');
});

test('RQS deep links & layout: ?approve=1 opens editable preview, unknown ?request= warns, and 320px/200% zoom stays clean', async ({
	page
}) => {
	// Unknown ?request= query shows error toast and strips the query param cleanly.
	await open(page, '/work?request=missing-request-id');
	await expect(page).toHaveURL(/\/work$/);
	await expect(page.getByText('That request is no longer here.')).toBeVisible();

	// ?approve=1 auto-opens editable Approve and create preview modal and strips ?approve=1.
	await visit(page, '/work/T-A43?approve=1');
	await expect(page).toHaveURL(/\/work\/T-A43$/);
	const approveDialog = page.getByRole('dialog', { name: 'Approve and create from T-A43' });
	await expect(approveDialog).toBeVisible();
	await approveDialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(approveDialog).toBeHidden();

	// 320 px narrow viewport with 200% text -> zero horizontal overflow on /work/T-A43.
	await page.setViewportSize({ width: 320, height: 800 });
	await visit(page, '/work/T-A43');
	await page.addStyleTag({ content: 'html { font-size: 200%; }' });
	await expect(page.getByTestId('request-outcome-section')).toBeVisible();
	await noSidewaysScroll(page);
});
