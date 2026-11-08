import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function createOrg(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
}
const rows = (page: Page) => page.getByTestId('agent-row');

test('AGENT-3: the Agents screen lists every agent A–Z with what it runs on, how it runs, status, teams, office and manager', async ({ page }) => {
	await createOrg(page);
	await expect(rows(page)).toHaveCount(8);
	const names = await rows(page).locator('th a').allInnerTexts();
	expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b)));
	const noor = page.locator('[data-testid=agent-row][data-agent=noor]');
	await expect(noor).toContainText('Noor Haddad');
	await expect(noor).toContainText('web engineer');
	await expect(noor).toContainText('Claude Code');
	await expect(noor).toContainText('On call');
	await expect(noor).toContainText('Active');
	await expect(noor).toContainText('Design, Web');
	await expect(noor).toContainText('HQ');
	await expect(noor).toContainText('Sam Okafor'); // reports to
	await expect(page.locator('[data-testid=agent-row][data-agent=ines]')).toContainText('Unplaced');
	await shot(page, 'agents-roster');
});

test('AGENT-3: filters by status, and an agent who left is listed only under Left', async ({ page }) => {
	await createOrg(page);
	await visit(page, '/org-chart');
	await page.locator('[data-testid=team-card][data-team=web]').getByRole('button', { name: /^Noor Haddad/ }).click();
	await page.getByTestId('selected-member').getByRole('button', { name: 'Pause…' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Pause Noor Haddad' }).click();
	await expect(page.getByText('Paused Noor Haddad.')).toBeVisible();
	await page.locator('[data-testid=team-card][data-team=eng]').getByRole('button', { name: /^Sam Okafor/ }).click();
	await page.getByTestId('selected-member').getByRole('button', { name: 'Offboard…' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Offboard Sam Okafor' }).click();
	await expect(page.getByText('Offboarded Sam Okafor.')).toBeVisible();

	await visit(page, '/agents');
	await expect(rows(page)).toHaveCount(7); // All leaves out the one who left
	await page.getByRole('button', { name: /^Paused/ }).click();
	await expect(rows(page)).toHaveCount(1);
	await expect(rows(page).first()).toContainText('Noor Haddad');
	await expect(rows(page).first()).toContainText('Paused');
	await page.getByRole('button', { name: /^Left/ }).click();
	await expect(rows(page)).toHaveCount(1);
	await expect(rows(page).first()).toContainText('Sam Okafor');
	await expect(rows(page).first()).toContainText('Left');
	await page.getByRole('button', { name: /^Active/ }).click();
	await expect(rows(page)).toHaveCount(6);
});

test('AGENT-3: a name opens the org chart with that agent selected', async ({ page }) => {
	await createOrg(page);
	await page.locator('[data-testid=agent-row][data-agent=kai] th a').click();
	await expect(page).toHaveURL(/\/org-chart\?member=kai$/);
	await expect(page.getByTestId('selected-member')).toContainText('Kai Lindqvist');
});

test('AGENT-4: Add agent from the Agents screen adds a row', async ({ page }) => {
	await createOrg(page);
	await page.getByRole('button', { name: 'Add agent' }).first().click();
	const sheet = page.getByRole('dialog', { name: 'Add an agent' });
	await sheet.getByLabel('Name', { exact: true }).fill('Cameron Díaz');
	await sheet.getByLabel('Role').fill('QA engineer');
	await sheet.getByRole('button', { name: 'Add agent' }).click();
	await expect(page.getByText('Added Cameron Díaz.')).toBeVisible();
	await expect(rows(page)).toHaveCount(9);
	await expect(page.locator('[data-testid=agent-row]', { hasText: 'Cameron Díaz' })).toContainText('cameron-diaz');
});

test('AGENT-3: an organization with no agents says so and offers Add your first agent', async ({ page }) => {
	await createOrg(page);
	await visit(page, '/agents?org=empty');
	await expect(page.getByTestId('no-agents')).toContainText('No agents yet');
	await page.getByRole('button', { name: 'Add your first agent' }).click();
	await expect(page.getByRole('dialog', { name: 'Add an agent' })).toBeVisible();
});
