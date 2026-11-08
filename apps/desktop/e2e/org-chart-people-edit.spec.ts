import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function openChart(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/org-chart');
	await expect(page.getByRole('tablist', { name: 'Org chart views' })).toBeVisible();
}
const card = (page: Page, id: string) => page.locator(`[data-testid=team-card][data-team=${id}]`);

test('PEOPLE-3: Add person takes a name, contact, teams and a manager; they appear in their teams and are selected', async ({ page }) => {
	await openChart(page);
	await page.getByRole('button', { name: 'Add person' }).first().click();
	const sheet = page.getByRole('dialog', { name: 'Add person' });
	await expect(sheet.getByRole('button', { name: 'Add person' })).toBeDisabled(); // no name yet
	await expect(sheet.getByLabel('Reports to')).toHaveValue(''); // the owner by default
	await expect(sheet.getByLabel('Reports to').locator('option').first()).toHaveText('You (you)');
	await sheet.getByLabel('Name').fill('Sam Ortiz');
	await sheet.getByLabel('Contact').fill('sam@example.com');
	await sheet.getByRole('checkbox', { name: 'Engineering › Web' }).click();
	await sheet.getByLabel('Reports to').selectOption({ label: 'Avery Stone' });
	await expect(sheet).toContainText('any agent in Sample organization can message them');
	await shot(page, 'add-person');
	await sheet.getByRole('button', { name: 'Add person' }).click();
	await expect(page.getByText('Added Sam Ortiz. Agents can now reach Sam Ortiz.')).toBeVisible();
	await expect(card(page, 'web').getByRole('button', { name: /^Sam Ortiz/ })).toBeVisible();
	await expect(page.getByTestId('selected-member')).toContainText('Sam Ortiz');
});

test('PEOPLE-4: a name that is already a handle is refused inline with a suggestion, and Add stays disabled', async ({ page }) => {
	await openChart(page);
	await page.getByRole('button', { name: 'Add person' }).first().click();
	const sheet = page.getByRole('dialog', { name: 'Add person' });
	await sheet.getByLabel('Name').fill('avery stone');
	await expect(sheet.getByRole('alert')).toHaveText('Avery Stone is already in Sample organization. Use a different name, such as avery stone (2).');
	await expect(sheet.getByRole('button', { name: 'Add person' })).toBeDisabled();
	await sheet.getByLabel('Name').fill('SAM');
	await expect(sheet.getByRole('alert')).toHaveText("SAM is an agent's name. Choose another.");
	await sheet.getByLabel('Name').fill('Owner');
	await expect(sheet.getByRole('alert')).toHaveText('"owner" is reserved for you. Choose another name.');
	await sheet.getByLabel('Name').fill('Avery Stone (Design)');
	await expect(sheet.getByRole('alert')).toHaveCount(0);
	await expect(sheet.getByRole('button', { name: 'Add person' })).toBeEnabled();
});

test('PEOPLE-3: a team’s menu starts Add person with the team ticked', async ({ page }) => {
	await openChart(page);
	await page.getByRole('button', { name: 'Actions for Design' }).click();
	await page.getByRole('menuitem', { name: 'Add person to Design…' }).click();
	await expect(page.getByRole('dialog', { name: 'Add person' }).getByRole('checkbox', { name: 'Design' })).toBeChecked();
});

test('PEOPLE-8: renaming checks the handle rule and says agents will use the new name; the contact can be cleared', async ({ page }) => {
	await openChart(page);
	await card(page, 'people').getByRole('button', { name: /^Hana Weiss/ }).click();
	await page.getByTestId('selected-member').getByRole('button', { name: 'Edit…' }).click();
	const dialog = page.getByRole('dialog', { name: 'Edit Hana Weiss' });
	await dialog.getByLabel('Name').fill('Leo Brandt');
	await expect(dialog.getByRole('alert')).toContainText('Leo Brandt is already in Sample organization');
	await expect(dialog.getByRole('button', { name: 'Save' })).toBeDisabled();
	await dialog.getByLabel('Name').fill('Hana W.');
	await expect(dialog.getByTestId('rename-note')).toHaveText('Agents will address them as Hana W. from now on.');
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(page.getByText('Saved Hana W..')).toBeVisible();
	await expect(card(page, 'people').getByRole('button', { name: /^Hana W\./ })).toBeVisible();
});

test('PEOPLE-5: removing a person confirms first, naming the teams, who reports to whom, and what agents can’t do', async ({ page }) => {
	await openChart(page);
	await card(page, 'people').getByRole('button', { name: /^Hana Weiss/ }).click();
	await page.getByTestId('selected-member').getByRole('button', { name: /^Remove from/ }).click();
	const dialog = page.getByRole('dialog', { name: 'Remove Hana Weiss from Sample organization?' });
	await expect(dialog).toContainText('Hana Weiss leaves People, and stops leading People.');
	await expect(dialog).toContainText("Inês Duarte and Leo Brandt, who report to Hana Weiss, will report to Hana Weiss's manager, Avery Stone.");
	await expect(dialog).toContainText("Messages waiting for Hana Weiss won't be delivered");
	await shot(page, 'remove-person');
	await dialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(card(page, 'people').getByRole('button', { name: /^Hana Weiss/ })).toBeVisible(); // nothing happened
	await page.getByTestId('selected-member').getByRole('button', { name: /^Remove from/ }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Remove Hana Weiss' }).click();
	await expect(page.getByText('Removed Hana Weiss.')).toBeVisible();
	await expect(card(page, 'people').getByRole('button', { name: /^Hana Weiss/ })).toHaveCount(0);
	await expect(card(page, 'people')).toContainText('No lead');
});

test('an agent can be selected but not edited or removed here', async ({ page }) => {
	await openChart(page);
	await card(page, 'platform').getByRole('button', { name: /^Kai Lindqvist/ }).click();
	const strip = page.getByTestId('selected-member');
	await expect(strip).toContainText('Kai Lindqvist');
	await expect(strip.getByRole('button', { name: 'Edit…' })).toHaveCount(0);
	await expect(strip.getByRole('button', { name: /^Remove from/ })).toHaveCount(0);
});

test('PEOPLE-3: the People tab has Add person too', async ({ page }) => {
	await openChart(page);
	await page.getByRole('tab', { name: 'People' }).click();
	await page.getByRole('button', { name: 'Add person' }).click();
	await expect(page.getByRole('dialog', { name: 'Add person' })).toBeVisible();
});
