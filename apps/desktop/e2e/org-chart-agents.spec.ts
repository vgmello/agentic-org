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
const strip = (page: Page) => page.getByTestId('selected-member');
async function select(page: Page, teamId: string, name: RegExp) {
	await card(page, teamId).getByRole('button', { name }).first().click();
}

test('AGENT-1: an agent’s quick view says its role, runtime, how it runs and its messaging name', async ({ page }) => {
	await openChart(page);
	await select(page, 'eng', /^Sam Okafor/);
	await expect(strip(page).getByTestId('agent-line')).toHaveText('software engineer · Runs on Claude Code · On call');
	await expect(strip(page).getByTestId('messaging-name')).toHaveText('sam');
	await shot(page, 'agent-quick-view');
});

test('DEAL-3: Pause confirms, then the agent shows Paused and Resume restores it', async ({ page }) => {
	await openChart(page);
	await select(page, 'web', /^Noor Haddad/);
	await strip(page).getByRole('button', { name: 'Pause…' }).click();
	const dialog = page.getByRole('dialog', { name: 'Pause Noor Haddad?' });
	await expect(dialog).toContainText("Noor Haddad won't be started or woken, and new tasks and messages wait until you resume.");
	await expect(dialog).toContainText('Noor Haddad stays in Design, Web.');
	await dialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(strip(page).getByTestId('paused-badge')).toHaveCount(0); // nothing happened
	await strip(page).getByRole('button', { name: 'Pause…' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Pause Noor Haddad' }).click();
	await expect(page.getByText('Paused Noor Haddad. Messages and tasks wait until you resume.')).toBeVisible();
	await expect(strip(page).getByTestId('paused-badge')).toBeVisible();
	await expect(card(page, 'web').getByRole('button', { name: /^Noor Haddad, agent, paused/ })).toBeVisible();
	await expect(card(page, 'web').getByTestId('paused-mark')).toHaveCount(1);
	await strip(page).getByRole('button', { name: 'Resume' }).click();
	await expect(page.getByText('Resumed Noor Haddad.')).toBeVisible();
	await expect(strip(page).getByTestId('paused-badge')).toHaveCount(0);
});

test('DEAL-4: Offboard names what changes, then the agent leaves the chart and is listed as a former member', async ({ page }) => {
	await openChart(page);
	await select(page, 'eng', /^Sam Okafor/);
	await strip(page).getByRole('button', { name: 'Offboard…' }).click();
	const dialog = page.getByRole('dialog', { name: 'Offboard Sam Okafor?' });
	await expect(dialog).toContainText('Sam Okafor leaves Engineering, and stops leading Engineering.');
	await expect(dialog).toContainText('open tasks Sam Okafor has come back unassigned');
	await expect(dialog).toContainText("Kai Lindqvist, Mira Castillo and Noor Haddad, who report to Sam Okafor, will report to Sam Okafor's manager, Avery Stone.");
	await expect(dialog).toContainText('the name sam stays reserved');
	await shot(page, 'offboard-agent');
	await dialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(card(page, 'eng').getByRole('button', { name: /^Sam Okafor/ })).toBeVisible(); // nothing happened
	await strip(page).getByRole('button', { name: 'Offboard…' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Offboard Sam Okafor' }).click();
	await expect(page.getByText('Offboarded Sam Okafor. The record and name stay.')).toBeVisible();
	await expect(page.getByRole('button', { name: /^Sam Okafor/ })).toHaveCount(0); // gone from the chart
	await expect(card(page, 'eng')).toContainText('No lead');
	const former = page.getByTestId('former-members');
	await expect(former).toContainText('Former members');
	await former.locator('summary').click();
	await expect(former).toContainText('Sam Okafor');
	await expect(former).toContainText('the name sam stays reserved');
	// Everyone who reported to Sam now reports to Avery, and the counts leave Sam out.
	await expect(page.getByText(/\b7\b.*agents/)).toBeVisible();
});

test('a messaging name stays reserved after the agent left: a person can’t take it', async ({ page }) => {
	await openChart(page);
	await select(page, 'eng', /^Sam Okafor/);
	await strip(page).getByRole('button', { name: 'Offboard…' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Offboard Sam Okafor' }).click();
	await expect(page.getByText('Offboarded Sam Okafor.')).toBeVisible();
	await page.getByRole('button', { name: 'Add person' }).first().click();
	await page.getByRole('dialog', { name: 'Add person' }).getByLabel('Name').fill('sam');
	await expect(page.getByRole('dialog', { name: 'Add person' }).getByRole('alert')).toHaveText("sam is an agent's name. Choose another.");
});

test('AGENT-2: Edit changes the record and refuses a messaging name that is taken', async ({ page }) => {
	await openChart(page);
	await select(page, 'eng', /^Sam Okafor/);
	await strip(page).getByRole('button', { name: 'Edit…' }).click();
	const dialog = page.getByRole('dialog', { name: 'Edit Sam Okafor' });
	await expect(dialog.getByLabel('Role')).toHaveValue('software engineer');
	await dialog.getByLabel('Title').fill('Principal engineer');
	await dialog.getByLabel('Focus').fill('the office server');
	await dialog.getByLabel('Runs on').selectOption('codex');
	await dialog.getByLabel('Messaging name').fill('noor');
	await expect(dialog.getByRole('alert')).toHaveText("noor is already Noor Haddad's messaging name. Choose another.");
	await expect(dialog.getByRole('button', { name: 'Save' })).toBeDisabled();
	await dialog.getByLabel('Messaging name').fill('sam-principal');
	await shot(page, 'edit-agent');
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(page.getByText('Saved Sam Okafor.')).toBeVisible();
	await expect(strip(page).getByTestId('agent-line')).toHaveText('Principal engineer · Runs on Codex · On call');
	await expect(strip(page).getByTestId('messaging-name')).toHaveText('sam-principal');
});

test('RUN-1: How it runs changes the mode; a scheduled agent needs a schedule; none means started by a person', async ({ page }) => {
	await openChart(page);
	await select(page, 'eng', /^Sam Okafor/);
	await strip(page).getByRole('button', { name: 'How it runs…' }).click();
	const dialog = page.getByRole('dialog', { name: 'How Sam Okafor runs' });
	await expect(dialog.getByRole('radio', { name: /^On call/ })).toBeChecked();
	await dialog.getByRole('radio', { name: /^Scheduled/ }).check();
	await expect(dialog.getByRole('button', { name: 'Save' })).toBeDisabled(); // a schedule is needed
	await dialog.getByLabel('When', { exact: true }).fill('weekdays at 9');
	await dialog.getByLabel('Time zone', { exact: true }).fill('Europe/Lisbon');
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(page.getByText('Saved Sam Okafor.')).toBeVisible();
	await expect(strip(page).getByTestId('agent-line')).toHaveText('software engineer · Runs on Claude Code · Scheduled');
	await strip(page).getByRole('button', { name: 'How it runs…' }).click();
	await page.getByRole('dialog').getByRole('radio', { name: /^Started by a person/ }).check();
	await page.getByRole('dialog').getByRole('button', { name: 'Save' }).click();
	await expect(strip(page).getByTestId('agent-line')).toHaveText('software engineer · Runs on Claude Code · Started by a person');
});

test('HIRE: Add agent adds an existing agent with a suggested messaging name, a team and a manager', async ({ page }) => {
	await openChart(page);
	await page.getByRole('button', { name: 'Add agent' }).click();
	const sheet = page.getByRole('dialog', { name: 'Add an agent' });
	await expect(sheet.getByRole('button', { name: 'Add agent' })).toBeDisabled();
	await sheet.getByLabel('Name', { exact: true }).fill('Cameron Díaz');
	await expect(sheet.getByLabel('Messaging name')).toHaveValue('cameron-diaz'); // follows the name
	await sheet.getByLabel('Role').fill('QA engineer');
	await sheet.getByLabel('Runs on').selectOption('codex');
	await sheet.getByRole('checkbox', { name: 'Engineering › Web' }).click();
	await sheet.getByLabel('Reports to').selectOption({ label: 'Sam Okafor' });
	await shot(page, 'add-agent');
	await sheet.getByRole('button', { name: 'Add agent' }).click();
	await expect(page.getByText('Added Cameron Díaz. Everyone in Acme can reach Cameron Díaz.')).toBeVisible();
	await expect(card(page, 'web').getByRole('button', { name: /^Cameron Díaz, agent/ })).toBeVisible();
	await expect(strip(page).getByTestId('agent-line')).toHaveText('QA engineer · Runs on Codex · On call');
});

test('HIRE: a messaging name that is taken is refused inline and Add stays disabled', async ({ page }) => {
	await openChart(page);
	await page.getByRole('button', { name: 'Add agent' }).click();
	const sheet = page.getByRole('dialog', { name: 'Add an agent' });
	await sheet.getByLabel('Name', { exact: true }).fill('Samira');
	await sheet.getByLabel('Role').fill('designer');
	await sheet.getByLabel('Messaging name').fill('SAM');
	await expect(sheet.getByRole('alert')).toHaveText("SAM is already Sam Okafor's messaging name. Choose another.");
	await expect(sheet.getByRole('button', { name: 'Add agent' })).toBeDisabled();
	await sheet.getByLabel('Messaging name').fill('-bad');
	await expect(sheet.getByRole('alert')).toContainText('Use 1 to 64 letters');
	await sheet.getByLabel('Messaging name').fill('samira');
	await expect(sheet.getByRole('alert')).toHaveCount(0);
	await expect(sheet.getByRole('button', { name: 'Add agent' })).toBeEnabled();
});

test('MOVE: Move to team moves an agent in one step, says what changes, and Undo puts it back', async ({ page }) => {
	await openChart(page);
	await select(page, 'web', /^Noor Haddad/);
	await strip(page).getByRole('button', { name: 'Move to team…' }).click();
	const dialog = page.getByRole('dialog', { name: 'Move Noor Haddad to a team' });
	await dialog.getByLabel('From').selectOption({ label: 'Web' });
	await expect(dialog.getByRole('radio', { name: /^Design/ })).toBeDisabled(); // Noor is already in Design
	await expect(dialog).toContainText('Noor Haddad is already in Design');
	await dialog.getByRole('radio', { name: /^Platform/ }).check();
	const summary = dialog.getByTestId('move-member-summary');
	await expect(summary).toContainText('Moves Noor Haddad from Web to Platform.');
	await expect(summary).toContainText("stops working on Web's projects (Beacon) and starts on Platform's (Atlas)");
	await expect(summary).toContainText('Noor Haddad leads Web. Web will have no lead.');
	await shot(page, 'move-member');
	await dialog.getByRole('button', { name: 'Move Noor Haddad' }).click();
	await expect(page.getByText('Noor Haddad moved to Platform.')).toBeVisible();
	await expect(card(page, 'platform').getByRole('button', { name: /^Noor Haddad/ })).toBeVisible();
	await expect(card(page, 'web').getByRole('button', { name: /^Noor Haddad/ })).toHaveCount(0);
	await expect(card(page, 'web')).toContainText('No lead');
	await page.getByRole('button', { name: 'Undo' }).click();
	await expect(page.getByText("Noor Haddad is back in Web. Noor Haddad doesn't lead it again: set the lead if you want that.")).toBeVisible(); // the lead isn't restored, and it says so
	await expect(card(page, 'web').getByRole('button', { name: /^Noor Haddad/ })).toBeVisible();
});

test('TEAM-11: Search members rings the matches and dims the rest, and the search stays when the tab changes', async ({ page }) => {
	await openChart(page);
	await page.getByLabel('Search members').fill('designer');
	await expect(page.getByTestId('search-status')).toContainText('2 matches');
	await expect(card(page, 'design').getByRole('button', { name: /^Juno Park, agent, matches the search/ })).toBeVisible();
	await expect(card(page, 'web').getByRole('button', { name: /^Noor Haddad, agent/ })).not.toHaveAttribute('aria-label', /matches/);
	await expect(card(page, 'web').getByRole('button', { name: /^Noor Haddad/ })).toHaveClass(/opacity-40/); // not a match: dimmed
	await page.getByRole('tab', { name: 'Reporting lines' }).click();
	await expect(page.getByLabel('Search members')).toHaveValue('designer'); // kept
	await expect(page.getByTestId('search-status')).toContainText('2 matches');
	await expect(page.getByRole('button', { name: /^Pax Moreau, agent, matches the search/ })).toBeVisible();
	await page.getByRole('button', { name: 'Clear' }).click();
	await expect(page.getByLabel('Search members')).toHaveValue('');
	await expect(page.getByTestId('search-status')).toHaveText('');
});

test('TEAM-11: a search with no match says so', async ({ page }) => {
	await openChart(page);
	await page.getByLabel('Search members').fill('zzzz');
	await expect(page.getByTestId('search-status')).toContainText('No one matches.');
});

test('DEAL-5: Show paused hides paused agents from the chart and counts, and brings them back', async ({ page }) => {
	await openChart(page);
	await expect(page.getByLabel('Show paused')).toHaveCount(0); // nobody is paused yet
	await select(page, 'web', /^Noor Haddad/);
	await strip(page).getByRole('button', { name: 'Pause…' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Pause Noor Haddad' }).click();
	await expect(page.getByText('Paused Noor Haddad.')).toBeVisible();
	const toggle = page.getByLabel('Show paused');
	await expect(toggle).toBeChecked();
	await toggle.uncheck();
	await expect(card(page, 'web').getByRole('button', { name: /^Noor Haddad/ })).toHaveCount(0);
	await expect(page.getByText(/\b7\b.*agents/)).toBeVisible(); // 8 agents, one hidden
	await toggle.check();
	await expect(card(page, 'web').getByRole('button', { name: /^Noor Haddad/ })).toBeVisible();
});

test('an offboarded agent is no longer counted on the Offices screen', async ({ page }) => {
	await openChart(page);
	await select(page, 'eng', /^Sam Okafor/);
	await strip(page).getByRole('button', { name: 'Offboard…' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Offboard Sam Okafor' }).click();
	await expect(page.getByText('Offboarded Sam Okafor.')).toBeVisible();
	await visit(page, '/offices'); // the mock office keeps the change in the browser's storage
	await expect(page.getByTestId('office-row').first()).toContainText('6 agents');
});

test('a stale edit: after HQ changed the agent, the dialog says so and a retry works, with what was typed kept', async ({ page }) => {
	await openChart(page);
	await select(page, 'eng', /^Sam Okafor/);
	// A first change, so the mock office has stored the organization (the browser mock keeps it in storage).
	await strip(page).getByRole('button', { name: 'Pause…' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Pause Sam Okafor' }).click();
	await strip(page).getByRole('button', { name: 'Resume' }).click();
	await expect(page.getByText('Resumed Sam Okafor.')).toBeVisible();
	await strip(page).getByRole('button', { name: 'Edit…' }).click();
	const dialog = page.getByRole('dialog', { name: 'Edit Sam Okafor' });
	await dialog.getByLabel('Title').fill('Principal engineer');
	// Someone else changes Sam meanwhile: the browser mock keeps the structure in storage, so change it there.
	await page.evaluate(() => {
		const key = Object.keys(localStorage).find((k) => k.startsWith('agentic-org.mock.org.'))!;
		const state = JSON.parse(localStorage.getItem(key)!);
		const sam = state.snapshot.members.find((m: { id: string }) => m.id === 'sam');
		sam.version += 1;
		localStorage.setItem(key, JSON.stringify(state));
	});
	// This one still reads the version it was opened with, so HQ refuses it.
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(dialog.getByRole('alert')).toContainText("changed at HQ while you were editing. Here's the latest.");
	await expect(dialog.getByLabel('Title')).toHaveValue('Principal engineer'); // what was typed is kept
	await dialog.getByRole('button', { name: 'Save' }).click(); // and the retry works
	await expect(page.getByText('Saved Sam Okafor.')).toBeVisible();
	await expect(strip(page).getByTestId('agent-line')).toContainText('Principal engineer');
});

test('TEAM-10: arrow keys move between the member tiles, Home and End go to the first and last', async ({ page }) => {
	await openChart(page);
	const tiles = page.locator('[data-member-tile]');
	const count = await tiles.count();
	await tiles.first().focus();
	await page.keyboard.press('ArrowRight');
	await expect(tiles.nth(1)).toBeFocused();
	await page.keyboard.press('ArrowDown');
	await expect(tiles.nth(2)).toBeFocused();
	await page.keyboard.press('ArrowLeft');
	await expect(tiles.nth(1)).toBeFocused();
	await page.keyboard.press('End');
	await expect(tiles.nth(count - 1)).toBeFocused();
	await page.keyboard.press('ArrowRight'); // the end stays the end
	await expect(tiles.nth(count - 1)).toBeFocused();
	await page.keyboard.press('Home');
	await expect(tiles.first()).toBeFocused();
	await page.keyboard.press('Enter'); // a tile is a button: Enter selects it
	await expect(page.getByTestId('selected-member')).toBeVisible();
});
