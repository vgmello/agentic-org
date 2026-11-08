import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

// The smart features on the TypeScript mock: "Draft or improve" on every text field, and "Report a
// bug from a sentence". `?copilot=off|failed|timeout` picks a scenario (src/lib/copilot/mock.ts).

async function openReport(page: Page, scenario = '') {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, `/report-bug${scenario ? `?copilot=${scenario}` : ''}`);
	await expect(page.getByRole('heading', { level: 1, name: 'Report a bug' })).toBeVisible();
}

const smartButtons = (page: Page) => page.getByRole('button', { name: /^Draft or improve/ });

test('the header link opens the report page, and says it is sample data', async ({ page }) => {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await page.getByRole('link', { name: 'Report a bug' }).click();
	await expect(page).toHaveURL(/\/report-bug$/);
	await expect(page.getByRole('heading', { level: 1, name: 'Report a bug' })).toBeVisible();
	await expect(page.getByTestId('sample-data-note')).toContainText('Sample data');
	await shot(page, 'report-bug-empty');
});

test('a sentence drafts the normal form, and nothing is reported until the person confirms', async ({ page }) => {
	await openReport(page);
	await page.getByLabel('What went wrong?').fill("The save button doesn't work after I rename a file. It just sits there.");
	await page.getByRole('button', { name: 'Draft the report' }).click();

	const banner = page.getByTestId('drafted-banner');
	await expect(banner).toContainText('Drafted by the copilot. Nothing is reported yet.');
	await expect(page.getByLabel('Title', { exact: true })).toHaveValue("The save button doesn't work after I rename a file");
	await expect(page.getByLabel('What happens')).toHaveValue(/sits there/);
	await expect(page.getByLabel('Severity')).toHaveValue('high');
	await expect(page.getByTestId('bug-sent')).toHaveCount(0);
	await shot(page, 'report-bug-drafted');

	// The person edits the draft, then confirms.
	await page.getByLabel('Title', { exact: true }).fill('Save does nothing after a rename');
	await page.getByRole('button', { name: 'Report bug' }).click();
	await expect(page.getByTestId('bug-sent')).toContainText('Bug reported');
	await expect(page.getByTestId('bug-id')).toHaveText('BUG-1');
	await expect(page.getByTestId('bug-sent')).toContainText('Save does nothing after a rename');
	await shot(page, 'report-bug-sent');

	await page.getByRole('button', { name: 'Report another bug' }).click();
	await expect(page.getByLabel('Title', { exact: true })).toHaveValue('');
});

test('undo draft brings back what was in the form', async ({ page }) => {
	await openReport(page);
	await page.getByLabel('Title', { exact: true }).fill('My own title');
	await page.getByLabel('What went wrong?').fill('It crashed on start');
	await page.getByRole('button', { name: 'Draft the report' }).click();
	await expect(page.getByLabel('Severity')).toHaveValue('critical');
	await expect(page.getByLabel('Title', { exact: true })).toHaveValue('It crashed on start');
	await page.getByRole('button', { name: 'Undo draft' }).click();
	await expect(page.getByLabel('Title', { exact: true })).toHaveValue('My own title');
	await expect(page.getByTestId('drafted-banner')).toHaveCount(0);
});

test('the form checks the required fields', async ({ page }) => {
	await openReport(page);
	await expect(page.getByRole('button', { name: 'Report bug' })).toBeDisabled();
	await page.getByLabel('What happens').fill('Nothing happens');
	await page.getByRole('button', { name: 'Report bug' }).click();
	await expect(page.getByText('Give the bug a title.')).toBeVisible();
	await expect(page.getByTestId('bug-sent')).toHaveCount(0);
});

test('Draft or improve is on every text field and replaces the text only when the person says so', async ({ page }) => {
	await openReport(page);
	await expect(smartButtons(page)).toHaveCount(4);
	const field = page.getByLabel('What happens');
	await field.fill('  the save  button does nothing');

	await page.getByRole('button', { name: 'Draft or improve What happens' }).click();
	const panel = page.getByTestId('copilot-bug-actual');
	await expect(panel).toContainText('Improve this text');
	await expect(panel.getByTestId('copilot-sample')).toHaveText('Sample data');
	await panel.getByRole('button', { name: 'Improve' }).click();
	await expect(panel.getByTestId('copilot-proposal')).toHaveText('The save button does nothing.');
	// It is only a proposal: the field still holds what the person wrote.
	await expect(field).toHaveValue('  the save  button does nothing');
	await shot(page, 'draft-or-improve-proposal');

	await panel.getByRole('button', { name: 'Use this draft' }).click();
	await expect(field).toHaveValue('The save button does nothing.');
	await expect(panel).toHaveCount(0);
	await page.getByRole('button', { name: 'Undo' }).click();
	await expect(field).toHaveValue('  the save  button does nothing');
});

test('Dismiss leaves the text alone, and an empty field gets a draft from an instruction', async ({ page }) => {
	await openReport(page);
	const field = page.getByLabel('What you expected');
	await page.getByRole('button', { name: 'Draft or improve What you expected' }).click();
	const panel = page.getByTestId('copilot-bug-expected');
	await expect(panel).toContainText('Draft this text');
	await panel.getByLabel('Instruction (optional)').fill('the file is saved');
	await panel.getByRole('button', { name: 'Draft', exact: true }).click();
	await expect(panel.getByTestId('copilot-proposal')).toContainText('The file is saved. [?]');
	await expect(panel.getByTestId('copilot-notes')).toContainText('Unknowns are marked [?]');
	await panel.getByRole('button', { name: 'Dismiss' }).click();
	await expect(field).toHaveValue('');
	await expect(panel).toHaveCount(0);
});

test('with nothing to go on, Draft asks for an instruction instead of running', async ({ page }) => {
	await openReport(page);
	await page.getByRole('button', { name: 'Draft or improve What you expected' }).click();
	const panel = page.getByTestId('copilot-bug-expected');
	await panel.getByRole('button', { name: 'Draft', exact: true }).click();
	await expect(panel.getByRole('alert')).toHaveText('Say what it should be about, then ask for a draft.');
	await expect(panel.getByTestId('copilot-proposal')).toHaveCount(0);
	await panel.getByLabel('Instruction (optional)').fill('the file is saved');
	await expect(panel.getByRole('alert')).toHaveCount(0);
});

test('reporting while a draft is on its way drops the draft', async ({ page }) => {
	await openReport(page);
	await page.getByLabel('Title', { exact: true }).fill('Typed title');
	await page.getByLabel('What happens').fill('Typed text');
	await page.getByLabel('What went wrong?').fill('It crashed on start');
	await page.getByRole('button', { name: 'Draft the report' }).click();
	await page.getByRole('button', { name: 'Report bug' }).click();
	await expect(page.getByTestId('bug-sent')).toContainText('Typed title');
	await page.getByRole('button', { name: 'Report another bug' }).click();
	// Longer than the mock's draft takes: the late draft must not fill the new, empty form.
	await page.waitForTimeout(1200);
	await expect(page.getByLabel('Title', { exact: true })).toHaveValue('');
	await expect(page.getByTestId('drafted-banner')).toHaveCount(0);
});

test('without a runtime the smart buttons are not there, and the form still works', async ({ page }) => {
	await openReport(page, 'off');
	await expect(page.getByLabel('Title', { exact: true })).toBeVisible();
	await expect(smartButtons(page)).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'Draft the report' })).toHaveCount(0);
	await expect(page.getByLabel('What went wrong?')).toHaveCount(0);
	await page.getByLabel('Title', { exact: true }).fill('Typed by hand');
	await page.getByLabel('What happens').fill('It breaks');
	await page.getByRole('button', { name: 'Report bug' }).click();
	await expect(page.getByTestId('bug-sent')).toContainText('Typed by hand');
	await shot(page, 'report-bug-no-copilot');
});

for (const [scenario, title] of [
	['failed', "The copilot couldn't draft this"],
	['timeout', 'The copilot took too long']
] as const) {
	test(`when the copilot ${scenario}, the person is told and nothing changes`, async ({ page }) => {
		await openReport(page, scenario);
		await page.getByLabel('What went wrong?').fill('It crashed');
		await page.getByRole('button', { name: 'Draft the report' }).click();
		await expect(page.getByRole('alert').filter({ hasText: title })).toBeVisible();
		await expect(page.getByLabel('Title', { exact: true })).toHaveValue('');
		await expect(page.getByTestId('drafted-banner')).toHaveCount(0);

		await page.getByLabel('What happens').fill('Some text');
		await page.getByRole('button', { name: 'Draft or improve What happens' }).click();
		const panel = page.getByTestId('copilot-bug-actual');
		await panel.getByRole('button', { name: 'Improve' }).click();
		await expect(panel.getByRole('alert').filter({ hasText: title })).toBeVisible();
		await expect(page.getByLabel('What happens')).toHaveValue('Some text');
		await shot(page, `copilot-${scenario}`);
	});
}
