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

// Cache the transformed test module before routing; no APIResponse outlives its test context.
async function mockOrgApi(page: Page, addition: () => string) {
	const response = await page.request.get('/e2e/fakes/org-api.ts');
	expect(response.status()).toBe(200);
	const source = await response.text();
	let calls = 0;
	await page.route('**/e2e/fakes/org-api.ts*', async (route) => {
		calls++;
		await route.fulfill({ status: 200, contentType: 'text/javascript', headers: { 'cache-control': 'no-store' }, body: source + '\n' + addition() });
	});
	return () => calls;
}

test.afterEach(async ({ page }) => {
	await page.unrouteAll({ behavior: 'wait' });
});


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
	await expect(noor.getByTestId('agent-status')).toHaveText(/Asleep · wakes when called/);
	await expect(noor.getByTestId('agent-now')).toHaveText('—');
	await expect(noor).not.toContainText('Active');
	await expect(noor).toContainText('Design, Web');
	await expect(noor).toContainText('HQ');
	await expect(noor).toContainText('Sam Okafor'); // reports to
	await expect(page.locator('[data-testid=agent-row][data-agent=ines]')).toContainText('Unplaced');
	await shot(page, 'agents-roster');
});

test('AGENT-3: filters by status, and an agent who left is listed only under Former', async ({ page }) => {
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
	await page.getByRole('button', { name: /^Former/ }).click();
	await expect(rows(page)).toHaveCount(1);
	await expect(rows(page).first()).toContainText('Sam Okafor');
	await expect(rows(page).first()).toContainText('Former');
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
	await shot(page, 'employees-empty');
	await page.getByRole('button', { name: 'Add your first agent' }).click();
	const sheet = page.getByRole('dialog', { name: 'Add an agent' });
	await expect(sheet).toBeVisible();
	await expect(sheet.getByRole('button', { name: 'Add agent' })).toBeDisabled();
	await expect(sheet.getByRole('list', { name: 'Teams' })).toHaveText('There are no teams yet.');
	await shot(page, 'employees-first-agent', { context: sheet });
});

test('AGENT-3 URL q: search uses trimmed name/title text and retains keyboard filtering', async ({ page }) => {
	await createOrg(page);
	await visit(page, '/agents?q=' + encodeURIComponent('  WeB EnGiNeEr  '));
	const search = page.getByLabel('Search agents', { exact: true });
	await expect(search).toHaveValue('WeB EnGiNeEr');
	await expect(rows(page)).toHaveCount(1);
	await expect(rows(page)).toContainText('Noor Haddad');
	await search.fill('noOR');
	await expect(rows(page)).toHaveCount(1);
	await expect(rows(page)).toContainText('Noor Haddad');
	await expect.poll(() => new URL(page.url()).searchParams.get('q')).toBe('noOR');
	await search.fill('Avery Stone');
	await expect(rows(page)).toHaveCount(0);
	await search.fill('   ');
	await expect(rows(page)).toHaveCount(8);
	await search.press('Tab');
	await expect(search).toHaveValue('');
	await search.fill('web');
	await search.press('End');
	await search.pressSequentially(' engineer');
	await expect(search).toHaveValue('web engineer');
	await expect(rows(page)).toHaveCount(1);
	await expect(search).toBeFocused();
	await shot(page, 'agents-query-name-title');
});

test('AGENT-4 URL add: opens once and clears the intent before refresh or Back', async ({ page }) => {
	await createOrg(page);
	await visit(page, '/agents?add=1');
	const dialog = page.getByRole('dialog', { name: 'Add an agent' });
	await expect(dialog).toBeVisible();
	await expect.poll(() => new URL(page.url()).searchParams.has('add')).toBe(false);
	await dialog.getByLabel('Name', { exact: true }).fill('Kept draft');
	await page.reload();
	await expect(dialog).toHaveCount(0);
	await expect(page.getByTestId('agent-roster')).toBeVisible();
	await page.getByRole('link', { name: 'Projects', exact: true }).click();
	await expect(page).toHaveURL(/\/projects$/);
	await page.goBack();
	await expect(page.getByTestId('agent-roster')).toBeVisible();
	await expect(dialog).toHaveCount(0);
	await shot(page, 'agents-add-intent-cleared');
});

test('AGENT-3 URL q: search is ANDed with status and URL edits replace history', async ({ page }) => {
	await createOrg(page);
	await visit(page, '/org-chart');
	await page.locator('[data-testid=team-card][data-team=web]').getByRole('button', { name: /^Noor Haddad/ }).click();
	await page.getByTestId('selected-member').getByRole('button', { name: 'Pause…' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Pause Noor Haddad' }).click();
	await expect(page.getByText('Paused Noor Haddad.')).toBeVisible();
	await visit(page, '/projects');
	await page.getByRole('link', { name: 'Agents', exact: true }).click();
	await expect(page).toHaveURL(/\/agents$/);
	const entries = await page.evaluate(() => history.length);
	const search = page.getByLabel('Search agents', { exact: true });
	await search.fill('noor');
	await expect.poll(() => new URL(page.url()).searchParams.get('q')).toBe('noor');
	await expect(rows(page)).toHaveCount(1);
	await page.getByRole('button', { name: /^Active / }).click();
	await expect(rows(page)).toHaveCount(0);
	await expect(page.getByTestId('no-match')).toHaveText('No agents match your search.');
	await page.getByRole('button', { name: /^Paused / }).click();
	await expect(rows(page)).toHaveCount(1);
	await expect(rows(page).getByTestId('agent-status')).toHaveText('Paused');
	expect(await page.evaluate(() => history.length)).toBe(entries);
	await page.goBack();
	await expect(page).toHaveURL(/\/projects$/);
	await page.goForward();
	await expect(search).toHaveValue('noor');
	await expect(rows(page)).toHaveCount(1);
	await page.reload();
	await expect(search).toHaveValue('noor');
	await expect(rows(page)).toHaveCount(1);
	await shot(page, 'agents-query-history-and-status');
});

test('AGENT-3 URL q: 200 and 201-character inputs and whitespace remain bounded', async ({ page }) => {
	await createOrg(page);
	for (const length of [200, 201]) {
		await visit(page, '/agents?q=' + 'a'.repeat(length));
		await expect(page.getByLabel('Search agents', { exact: true })).toHaveValue('a'.repeat(200));
		await expect(rows(page)).toHaveCount(0);
	}
	const bounded = page.getByLabel('Search agents', { exact: true });
	await bounded.fill('b'.repeat(201));
	await expect(bounded).toHaveValue('b'.repeat(200));
	await expect.poll(() => new URL(page.url()).searchParams.get('q')).toBe('b'.repeat(200));
	await visit(page, '/agents?q=' + encodeURIComponent(' \t\n '));
	await expect(page.getByLabel('Search agents', { exact: true })).toHaveValue('');
	await expect(rows(page)).toHaveCount(8);
	await shot(page, 'agents-query-whitespace');
});

test('AGENT-4 URL add: Escape restores Add focus and invalid values do not open', async ({ page }) => {
	await createOrg(page);
	await visit(page, '/agents?q=Noor&add=1#roster');
	const dialog = page.getByRole('dialog', { name: 'Add an agent' });
	await expect(dialog).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(dialog).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'Add agent', exact: true })).toBeFocused();
	expect(new URL(page.url()).searchParams.get('q')).toBe('Noor');
	expect(new URL(page.url()).hash).toBe('#roster');
	for (const parameters of ['add=0', 'add=', 'add=true', 'add=1&add=1', 'add=1&add=0']) {
		await visit(page, '/agents?' + parameters);
		await expect(page.getByTestId('agent-roster')).toBeVisible();
		await expect.poll(() => new URL(page.url()).searchParams.has('add')).toBe(false);
		await expect(dialog).toHaveCount(0);
	}
	await shot(page, 'agents-add-invalid');
});

test('AGENT-4 URL add: an already-open dialog keeps its draft on a repeated intent', async ({ page }) => {
	await createOrg(page);
	const search = page.getByLabel('Search agents', { exact: true });
	await search.fill('Noor');
	await expect.poll(() => new URL(page.url()).searchParams.get('q')).toBe('Noor');
	await page.getByRole('button', { name: 'Add agent', exact: true }).click();
	const dialog = page.getByRole('dialog', { name: 'Add an agent' });
	await dialog.getByLabel('Name', { exact: true }).fill('Existing draft');
	// A test-owned link exercises real same-page router navigation while the dialog is open.
	await dialog.evaluate((element) => {
		const link = document.createElement('a');
		link.href = '/agents?add=1'; link.textContent = 'Repeat Add intent';
		element.prepend(link);
	});
	const entries = await page.evaluate(() => history.length);
	await dialog.getByRole('link', { name: 'Repeat Add intent' }).click();
	await expect.poll(() => page.evaluate(() => history.length)).toBe(entries + 1);
	await expect.poll(() => new URL(page.url()).searchParams.has('add')).toBe(false);
	await expect(dialog).toHaveCount(1);
	await expect(dialog.getByLabel('Name', { exact: true })).toHaveValue('Existing draft');
	await shot(page, 'agents-add-existing-draft', { context: dialog });
	await page.keyboard.press('Escape');
	await expect(dialog).toHaveCount(0);
	await expect(search).toHaveValue('');
	await expect(rows(page)).toHaveCount(8);
});

test('AGENT-4 URL add: an unavailable shell defers the intent until recovery', async ({ page }) => {
	await createOrg(page);
	let intercepted = 0;
	const pattern = '**/e2e/fakes/capabilities.ts*';
	await page.route(pattern, async (route) => {
		intercepted++;
		await route.fulfill({ contentType: 'text/javascript', headers: { 'cache-control': 'no-store' }, body: 'export const areaCapability = () => ({ available: false, reason: "unavailable", message: "Unavailable" });' });
	});
	await visit(page, '/agents?add=1');
	await expect(page.getByTestId('area-unavailable')).toBeVisible();
	await expect(page.getByRole('dialog')).toHaveCount(0);
	expect(new URL(page.url()).searchParams.get('add')).toBe('1');
	expect(intercepted).toBeGreaterThan(0);
	await shot(page, 'agents-add-unavailable-deferred');
	await page.unroute(pattern);
	await page.reload();
	const dialog = page.getByRole('dialog', { name: 'Add an agent' });
	await expect(dialog).toBeVisible();
	await expect.poll(() => new URL(page.url()).searchParams.has('add')).toBe(false);
	await page.reload();
	await expect(dialog).toHaveCount(0);
});

test('AGENT-4 URL add: a load error defers intent and recovery consumes it once', async ({ page }) => {
	await createOrg(page);
	let failing = true;
	const intercepted = await mockOrgApi(page, () => failing ? 'orgApi.view = async () => { throw { source: "api", code: "storage_error", message: "Office unavailable for test" }; };' : '');
	await visit(page, '/agents?add=1');
	await expect(page.getByText('Office unavailable for test', { exact: true })).toBeVisible();
	await expect(page.getByRole('dialog')).toHaveCount(0);
	expect(new URL(page.url()).searchParams.get('add')).toBe('1');
	expect(intercepted()).toBeGreaterThan(0);
	await shot(page, 'agents-add-load-error-deferred');
	failing = false;
	await page.reload();
	const dialog = page.getByRole('dialog', { name: 'Add an agent' });
	await expect(dialog).toBeVisible();
	await expect.poll(() => new URL(page.url()).searchParams.has('add')).toBe(false);
	await page.reload();
	await expect(dialog).toHaveCount(0);
});

test('AGENT-4 URL add: HQ refusal keeps the existing dialog error and entered values', async ({ page }) => {
	await createOrg(page);
	const intercepted = await mockOrgApi(page, () => 'orgApi.command = async () => { throw { source: "api", code: "not_authorized", message: "HQ refused this Add" }; };');
	await visit(page, '/agents?add=1');
	const dialog = page.getByRole('dialog', { name: 'Add an agent' });
	await expect(dialog).toBeVisible();
	await dialog.getByLabel('Name', { exact: true }).fill('Test Agent');
	await dialog.getByLabel('Role', { exact: true }).fill('Tester');
	await dialog.getByRole('button', { name: 'Add agent', exact: true }).click();
	await expect(dialog.getByText('HQ refused this Add', { exact: true })).toBeVisible();
	await expect(dialog.getByLabel('Name', { exact: true })).toHaveValue('Test Agent');
	expect(intercepted()).toBeGreaterThan(0);
	await expect(rows(page)).toHaveCount(8);
	await shot(page, 'agents-add-hq-refusal', { context: dialog });
});

test('AGENT-3/4 URL input: replacements preserve focus and never refetch the roster', async ({ page }) => {
	await createOrg(page);
	const intercepted = await mockOrgApi(page, () => 'const originalViewForTest = orgApi.view.bind(orgApi); orgApi.view = (...args) => { window.__rosterReads = (window.__rosterReads || 0) + 1; return originalViewForTest(...args); };');
	await visit(page, '/agents?q=&add=1#agents-query');
	const dialog = page.getByRole('dialog', { name: 'Add an agent' });
	await expect(dialog).toBeVisible();
	await expect.poll(() => new URL(page.url()).searchParams.has('add')).toBe(false);
	const reads = () => page.evaluate(() => (window as unknown as { __rosterReads: number }).__rosterReads);
	expect(await reads()).toBe(1);
	expect(intercepted()).toBeGreaterThan(0);
	await page.keyboard.press('Escape');
	const search = page.getByLabel('Search agents', { exact: true });
	const main = page.getByRole('main');
	await main.evaluate((element) => { element.scrollTop = 20; });
	await search.focus();
	const scrollBefore = await main.evaluate((element) => element.scrollTop);
	expect(scrollBefore).toBeGreaterThan(0);
	await search.pressSequentially(' ');
	await expect.poll(() => new URL(page.url()).searchParams.has('q')).toBe(false);
	expect(new URL(page.url()).hash).toBe('#agents-query');
	expect(await main.evaluate((element) => element.scrollTop)).toBe(scrollBefore);
	expect(await reads()).toBe(1);
	await search.fill('');
	await search.pressSequentially('web engineer');
	await expect.poll(() => new URL(page.url()).searchParams.get('q')).toBe('web engineer');
	await expect(rows(page)).toHaveCount(1);
	await expect(search).toBeFocused();
	expect(await reads()).toBe(1);
	await search.fill('');
	await expect.poll(() => new URL(page.url()).searchParams.has('q')).toBe(false);
	await expect(rows(page)).toHaveCount(8);
	await expect(search).toBeFocused();
	expect(await reads()).toBe(1);
	await shot(page, 'agents-query-no-refetch');
});

for (const width of [320, 640, 960, 1280]) for (const scale of [1, 1.25, 2]) {
	test('AGENT-3/4 URL preflight: query and dialog states at ' + width + 'px and ' + scale * 100 + '% text', async ({ page }) => {
		await createOrg(page);
		const size = async () => {
			await page.setViewportSize({ width, height: 900 });
			await page.locator('html').evaluate((html, pixels) => { html.style.fontSize = pixels + 'px'; }, 16 * scale);
		};
		await size();
		expect((await page.request.get('/e2e/fakes/org-api.ts')).status()).toBe(200);
		const search = page.getByLabel('Search agents', { exact: true });
		await search.scrollIntoViewIfNeeded();
		const fits = await search.evaluate((input) => {
			const box = input.getBoundingClientRect();
			return box.left >= 0 && box.right <= innerWidth && box.width > 0;
		});
		expect(fits).toBe(true);
		await expect(rows(page)).toHaveCount(8);
		await shot(page, 'agents-query-' + width + '-' + scale + '-all', { context: search.locator('..') });
		await search.fill('Noor');
		await expect.poll(() => new URL(page.url()).searchParams.get('q')).toBe('Noor');
		await expect(rows(page)).toHaveCount(1);
		await expect(search).toBeFocused();
		await expect(page.getByRole('table', { name: 'Agents', exact: true }).getByRole('columnheader')).toHaveText(['Agent', 'Runs on', 'How it runs', 'Status', 'Now', 'Teams', 'Office', 'Reports to']);
		await shot(page, 'agents-query-' + width + '-' + scale + '-match');
		await search.fill('no-such-agent');
		await expect(page.getByTestId('no-match')).toHaveText('No agents match your search.');
		await shot(page, 'agents-query-' + width + '-' + scale + '-no-match', { context: page.getByTestId('no-match') });
		await visit(page, '/agents?q=Noor&add=1');
		await size();
		const dialog = page.getByRole('dialog', { name: 'Add an agent' });
		await expect(dialog).toBeVisible();
		await expect.poll(() => new URL(page.url()).searchParams.has('add')).toBe(false);
		await dialog.getByLabel('Name', { exact: true }).focus();
		await expect(dialog.getByLabel('Name', { exact: true })).toBeFocused();
		await shot(page, 'agents-query-' + width + '-' + scale + '-dialog', { context: dialog });
		await page.keyboard.press('Escape');
		await expect(dialog).toHaveCount(0);
		await expect(page.getByRole('button', { name: 'Add agent', exact: true })).toBeFocused();
		await visit(page, '/agents?org=empty&q=Noor');
		await size();
		await expect(page.getByTestId('no-agents')).toContainText('No agents yet');
		await expect(search).toHaveValue('Noor');
		await expect(rows(page)).toHaveCount(0);
		await shot(page, 'agents-query-' + width + '-' + scale + '-empty', { context: page.getByTestId('no-agents') });
	});
}

test('AGENT-4 URL add: the existing Add action is disabled during a pending navigation', async ({ page }) => {
	await createOrg(page);
	await mockOrgApi(page, () => 'const originalHeldViewForTest = orgApi.view.bind(orgApi); orgApi.view = async (...args) => { if (window.__holdRosterView) await window.holdRosterForTest(); return originalHeldViewForTest(...args); };');
	await visit(page, '/agents');
	let release!: () => void;
	const gate = new Promise<void>((resolve) => { release = resolve; });
	let held = 0;
	await page.exposeFunction('holdRosterForTest', async () => {
		held++;
		await gate;
	});
	await page.evaluate(() => { (window as unknown as { __holdRosterView: boolean }).__holdRosterView = true; });
	const navigation = page.getByRole('link', { name: 'Projects', exact: true }).click();
	try {
		await expect.poll(() => held).toBeGreaterThan(0);
		await expect(page.getByRole('main', { includeHidden: true }).getByRole('button', { name: 'Add agent', exact: true, includeHidden: true })).toBeDisabled();
		await expect(page.getByRole('dialog')).toHaveCount(0);
		await shot(page, 'agents-add-navigation-pending');
	} finally {
		release();
		await navigation;
	}
	await expect(page).toHaveURL(/\/projects$/);
	await page.getByRole('link', { name: 'Agents', exact: true }).click();
	await expect(page.getByRole('button', { name: 'Add agent', exact: true })).toBeEnabled();
});

test('AGENT-4 URL focus: a pending navigation returns URL-dialog focus to enabled search', async ({ page }) => {
	await createOrg(page);
	await mockOrgApi(page, () => 'const pendingFocusView = orgApi.view.bind(orgApi); orgApi.view = async (...args) => { if (window.__holdRosterView) await window.holdRosterForTest(); return pendingFocusView(...args); };');
	await visit(page, '/agents?add=1');
	const dialog = page.getByRole('dialog', { name: 'Add an agent' });
	await expect(dialog).toBeVisible();
	await expect.poll(() => new URL(page.url()).searchParams.has('add')).toBe(false);
	let release!: () => void;
	const gate = new Promise<void>((resolve) => { release = resolve; });
	let held = 0;
	await page.exposeFunction('holdRosterForTest', async () => {
		held++;
		await gate;
	});
	await page.evaluate(() => { (window as unknown as { __holdRosterView: boolean }).__holdRosterView = true; });
	await dialog.evaluate((element) => {
		const link = document.createElement('a');
		link.href = '/projects'; link.textContent = 'Navigate to Projects'; element.prepend(link);
	});
	const navigation = dialog.getByRole('link', { name: 'Navigate to Projects' }).click();
	try {
		await expect.poll(() => held).toBeGreaterThan(0);
		await expect(page.getByRole('main', { includeHidden: true }).getByRole('button', { name: 'Add agent', exact: true, includeHidden: true })).toBeDisabled();
		await page.keyboard.press('Escape');
		await expect(dialog).toHaveCount(0);
		await expect(page.getByLabel('Search agents', { exact: true })).toBeFocused();
		await shot(page, 'agents-add-close-while-navigation-pending');
	} finally {
		release();
		await navigation;
	}
});

test('AGENT-4 legacy focus: empty-organization Add keeps its original opener', async ({ page }) => {
	await createOrg(page);
	await visit(page, '/agents?org=empty');
	const opener = page.getByRole('button', { name: 'Add your first agent', exact: true });
	await opener.focus(); await opener.press('Enter');
	await expect(page.getByRole('dialog', { name: 'Add an agent' })).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(page.getByRole('dialog')).toHaveCount(0);
	await expect(opener).toBeFocused();
	await shot(page, 'agents-add-empty-opener-focus');
});

test('AGENT-3 query field: design-system v1 boundary and keyboard outline are visible', async ({ page }) => {
	await createOrg(page);
	const input = page.getByLabel('Search agents', { exact: true });
	const colors = await page.evaluate(() => {
		const reference = document.createElement('span'); reference.style.display = 'none';
		document.body.append(reference);
		reference.style.color = 'var(--color-gray-500)'; const boundary = getComputedStyle(reference).color;
		reference.style.color = 'var(--color-indigo-400)'; const focus = getComputedStyle(reference).color;
		reference.remove(); return { boundary, focus };
	});
	const style = () => input.evaluate((element) => {
		const css = getComputedStyle(element);
		return { style: css.outlineStyle, width: css.outlineWidth, color: css.outlineColor, offset: parseFloat(css.outlineOffset), border: css.borderTopWidth };
	});
	await expect(input).toHaveAccessibleDescription('Name or role title');
	const idle = await style();
	expect.soft(idle.style).toBe('solid'); expect.soft(idle.width).toBe('1px');
	expect.soft(idle.color).toBe(colors.boundary); expect.soft(idle.border).toBe('0px');
	await input.focus();
	const focused = await style();
	expect.soft(focused.style).toBe('solid'); expect.soft(focused.width).toBe('2px');
	expect.soft(focused.color).toBe(colors.focus); expect.soft(focused.offset).toBeLessThanOrEqual(0);
	await shot(page, 'agents-query-design-system-focus');
});

test('AGENT-3 URL q: a missing optional role title still supports name search', async ({ page }) => {
	await createOrg(page);
	const intercepted = await mockOrgApi(page, () => 'const optionalTitleView = orgApi.view.bind(orgApi); orgApi.view = async (...args) => { const view = structuredClone(await optionalTitleView(...args)); delete view.snapshot.members.find((member) => member.id === "noor").agent.title; return view; };');
	await visit(page, '/agents?q=web%20engineer');
	const search = page.getByLabel('Search agents', { exact: true });
	await expect(page.getByTestId('no-match')).toHaveText('No agents match your search.');
	await expect(page.getByRole('heading', { name: 'Something went wrong' })).toHaveCount(0);
	expect(intercepted()).toBeGreaterThan(0);
	await search.fill('Noor');
	await expect(rows(page)).toHaveCount(1);
	await expect(rows(page)).toContainText('Noor Haddad');
	await shot(page, 'agents-query-optional-title');
});
