import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { signInAsAdmin } from './web-session';
import { enterMockedOrganization } from './mock-organization';

// Organization settings › Roles against the mock roles API (roles-ux §1, ROLE-1, ROLE-5, ROLE-9, ROLE-10).
async function openOrganization(page: Page) {
	await signInAsAdmin(page, [
		{
			id: 'org-roles',
			name: 'Lumen Labs',
			created_at: '2026-11-18T00:00:00Z',
			recovery: 'acknowledged'
		}
	]);
	await enterMockedOrganization(page, 'Lumen Labs');
}

const rows = (page: Page) => page.locator('main ul[aria-labelledby="roles-heading"] > li');
const heading = (page: Page) => page.locator('#role-heading');
const editor = (page: Page) => page.getByRole('dialog');

async function noSidewaysScroll(page: Page) {
	const overflow = await page.evaluate(
		() => document.documentElement.scrollWidth - document.documentElement.clientWidth
	);
	expect(overflow).toBeLessThanOrEqual(0);
}

test('RL-1, RL-4: the list shows each role with its revision and employees, and a role page shows its parts', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/roles');
	await expect(page).toHaveTitle('Roles · Organization settings · Agentic Org');
	await expect(page.getByRole('heading', { level: 2, name: /^Roles/ })).toContainText('4');
	await expect(rows(page)).toHaveCount(4);
	await expect(rows(page).nth(0)).toContainText('Product Designer');
	await expect(rows(page).nth(0)).toContainText('no employees yet');
	await expect(rows(page).nth(1)).toContainText('QA Engineer');
	await expect(rows(page).nth(1)).toContainText('no employees yet');
	await expect(rows(page).nth(2)).toContainText('Software Engineer');
	await expect(rows(page).nth(2)).toContainText('rev 3');
	await expect(rows(page).nth(2)).toContainText('2 employees');

	await page.getByRole('link', { name: 'Software Engineer' }).click();
	await expect(page).toHaveURL(/\/settings\/roles\/role-swe$/);
	await expect(heading(page)).toHaveText('Software Engineer');
	// After a client move the role's heading takes focus once it has loaded (§6a).
	await expect(heading(page)).toBeFocused();
	await expect(page).toHaveTitle('Software Engineer · Roles · Organization settings · Agentic Org');
	await expect(page.locator('main header').first()).toContainText('software-engineer');
	await expect(page.locator('main header').first()).toContainText('rev 3');
	const body = page.getByTestId('role-body');
	// The text has its own h3; its sections are h4 under it, apart from the page's h3 sections (P3-4).
	await expect(body.getByRole('heading', { level: 3, name: 'Role text' })).toBeAttached();
	await expect(body.getByRole('heading', { level: 4, name: 'Mission' })).toBeVisible();
	await expect(body.getByRole('heading', { level: 4, name: 'Working style' })).toBeVisible();
	await expect(body.getByRole('heading', { level: 3 })).toHaveCount(1);
	await expect(body).toContainText('Small changes, each with its reason.');
	// The same employees as their profiles' Role tab (members mock): Mira and Noor.
	const employees = page.getByTestId('role-employees');
	await expect(employees.locator('li')).toHaveCount(2);
	await expect(employees).toContainText('Mira Castillo');
	await expect(employees).toContainText('Agent');
	await expect(employees).not.toContainText('Leo Brandt');
	await expect(employees.getByRole('link', { name: 'Noor Haddad' })).toHaveAttribute(
		'href',
		'/members/noor'
	);
	const history = page.getByTestId('role-history');
	await expect(history.locator('> li')).toHaveCount(3);
	await expect(history.locator('> li').first()).toContainText('Current');
	await expect(history.locator('> li').first()).toContainText('Alex Kim');
	await expect(history.locator('> li').last()).toContainText('Agentic Org');
	await history.getByText('Text of rev 1').click();
	await expect(history).toContainText('Implement and review assigned work.');
	await expect(page.getByTestId('key-lock')).toHaveText(
		"Feature delivery uses this key, so it can't be changed or removed."
	);
	await expect(page.getByText('roles/role-swe.md')).toBeVisible();

	await page.locator('main header').getByRole('link', { name: 'Roles', exact: true }).click();
	await expect(page).toHaveURL(/\/settings\/roles$/);
});

test('RL-2: New role checks its fields, keeps them on a refusal and opens the new role', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/roles');
	const opener = page.getByRole('button', { name: 'New role…' });
	await opener.click();
	await expect(editor(page)).toBeVisible();
	await expect(editor(page).getByRole('heading', { name: 'New role' })).toBeVisible();
	await expect(page.getByLabel('Text', { exact: true })).toHaveValue(
		/## Mission[\s\S]*## Boundaries/
	);

	await editor(page).getByRole('button', { name: 'Create role' }).click();
	await expect(page.getByLabel('Name')).toBeFocused();
	await expect(page.getByLabel('Name')).toHaveAttribute('aria-invalid', 'true');
	await expect(editor(page)).toContainText('Give the role a name.');

	await page.getByLabel('Name').fill('Release Manager');
	await page.getByLabel('Key (optional)').fill('qa-engineer');
	await page.getByLabel('Text', { exact: true }).fill('## Mission\nKeep releases moving.\n');
	await editor(page).getByRole('button', { name: 'Create role' }).click();
	await expect(editor(page)).toContainText('Another role already uses the key qa-engineer.');
	await expect(page.getByLabel('Key (optional)')).toBeFocused();
	await expect(page.getByLabel('Name')).toHaveValue('Release Manager');
	await expect(page.getByLabel('Text', { exact: true })).toHaveValue(
		'## Mission\nKeep releases moving.\n'
	);

	await page.getByLabel('Key (optional)').fill('Release Manager');
	await editor(page).getByRole('button', { name: 'Create role' }).click();
	await expect(editor(page)).toBeHidden();
	await expect(heading(page)).toHaveText('Release Manager');
	await expect(heading(page)).toBeFocused();
	await expect(page.locator('main header').first()).toContainText('release-manager');
	await expect(page.locator('main header').first()).toContainText('rev 1');
	await expect(page.getByTestId('role-body')).toContainText('Keep releases moving.');
	await expect(page.locator('main')).toContainText('No employees have this role yet.');

	await page.locator('main header').getByRole('link', { name: 'Roles', exact: true }).click();
	await expect(rows(page)).toHaveCount(5);
	await expect(rows(page).filter({ hasText: 'Release Manager' })).toContainText('no employees yet');
});

test('RL-5: Edit says what saving does, saves the next revision and returns focus', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/roles/role-staff');
	await page.getByRole('button', { name: 'Edit' }).click();
	await expect(editor(page).getByRole('heading', { name: 'Edit Staff Engineer' })).toBeVisible();
	await expect(page.getByTestId('save-effect')).toHaveText(
		'Saving makes rev 2. The 1 employee with this role read the new text.'
	);
	// Nothing changed: the sheet says so and stays open; no revision is added (P3-3).
	await editor(page).getByRole('button', { name: 'Save role' }).click();
	await expect(page.getByTestId('save-notice')).toHaveText(
		"Nothing changed, so there's nothing to save."
	);
	await expect(editor(page)).toBeVisible();
	await editor(page).getByRole('button', { name: 'Cancel' }).click();
	await expect(editor(page)).toBeHidden();
	await expect(page.getByRole('button', { name: 'Edit' })).toBeFocused();
	await expect(page.locator('main header').first()).toContainText('rev 1');

	await page.getByRole('button', { name: 'Edit' }).click();
	await page.getByLabel('Name').fill('Principal Engineer');
	await editor(page).getByRole('button', { name: 'Save role' }).click();
	await expect(editor(page)).toBeHidden();
	await expect(heading(page)).toHaveText('Principal Engineer');
	await expect(page.locator('main header').first()).toContainText('rev 2');
	await expect(page.getByText('Principal Engineer saved as rev 2.')).toBeVisible();
	await expect(page.getByTestId('role-history').locator('> li').first()).toContainText(
		'Renamed from Staff Engineer'
	);
	await expect(page.getByRole('button', { name: 'Edit' })).toBeFocused();
});

test('RL-6: a revision conflict keeps the input, shows the newer version and saves over it', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/roles/role-designer?mock=conflict');
	await page.getByRole('button', { name: 'Edit' }).click();
	await page.getByLabel('Text', { exact: true }).fill('## Mission\nMy own words.\n');
	await editor(page).getByRole('button', { name: 'Save role' }).click();
	const conflict = page.getByTestId('conflict');
	// The notice sits at the top of the sheet, away from Save: it takes focus so it is seen and read.
	await expect(page.locator('#role-conflict')).toBeFocused();
	await expect(conflict).toContainText(
		"This role changed while you were editing. Here's the newer version; your changes are kept."
	);
	await expect(conflict).toContainText('Newer version: rev 3, Product Designer');
	await expect(conflict).toContainText('Updated by Avery while you were editing.');
	await expect(page.getByLabel('Text', { exact: true })).toHaveValue('## Mission\nMy own words.\n');
	await expect(page.getByTestId('save-effect')).toContainText('Saving makes rev 4.');
	await editor(page).getByRole('button', { name: 'Save over rev 3' }).click();
	await expect(editor(page)).toBeHidden();
	await expect(page.locator('main header').first()).toContainText('rev 4');
	await expect(page.getByTestId('role-body')).toContainText('My own words.');
});

test(
	'RL-7: a key a development process names cannot be changed',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOrganization(page);
		await visit(page, '/settings/roles/role-swe');
		await page.getByRole('button', { name: 'Edit' }).click();
		const key = page.getByLabel('Key (optional)');
		await expect(key).toHaveAttribute('readonly', '');
		await expect(key).toHaveAccessibleDescription(
			"Feature delivery uses this key, so it can't be changed or removed."
		);
		await page.keyboard.press('Escape');
		await expect(editor(page)).toBeHidden();
		await expect(page.getByRole('button', { name: 'Edit' })).toBeFocused();
	}
);

test('RL-3: the HR lead drafts a role for review, and is disabled with the reason when it cannot run', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/roles');
	await page.getByRole('button', { name: 'Ask HR to draft…' }).click();
	const ask = page.getByRole('dialog');
	await ask.getByRole('button', { name: 'Ask HR' }).click();
	await expect(page.getByLabel('What should this role do?')).toBeFocused();
	await page.getByLabel('What should this role do?').fill('release coordinator for the web app');
	await ask.getByRole('button', { name: 'Ask HR' }).click();
	await expect(ask.getByRole('button', { name: 'Drafting…' })).toBeVisible();
	await expect(editor(page).getByRole('heading', { name: 'New role' })).toBeVisible();
	await expect(page.getByTestId('hr-draft-note')).toContainText('Drafted by the HR lead.');
	await expect(page.getByLabel('Name')).toHaveValue('Release Coordinator For');
	await expect(page.getByLabel('Text', { exact: true })).toHaveValue(
		/release coordinator for the web app/
	);
	await editor(page).getByRole('button', { name: 'Cancel' }).click();
	await expect(editor(page)).toBeHidden();
	await expect(page.getByRole('button', { name: 'Ask HR to draft…' })).toBeFocused();
	await expect(rows(page)).toHaveCount(4);

	await visit(page, '/settings/roles?mock=hr-off');
	const button = page.getByRole('button', { name: 'Ask HR to draft…' });
	await expect(button).toBeDisabled();
	await expect(button).toHaveAccessibleDescription(
		"The HR lead can't run right now, so it can't draft roles."
	);
	await expect(page.getByTestId('hr-reason')).toBeVisible();

	// Empty copy never promises that HR can run (ROLE-10).
	await visit(page, '/settings/roles?mock=empty,hr-off');
	await expect(page.getByRole('heading', { name: 'No roles yet' })).toBeVisible();
	await expect(page.locator('main')).toContainText("Roles describe employees' work. Create one.");
	await expect(page.locator('main')).not.toContainText('ask HR to draft one');
	await visit(page, '/settings/roles?mock=empty');
	await expect(page.locator('main')).toContainText(
		"Roles describe employees' work. Create one, or ask HR to draft one."
	);
});

test('RL-8: loading, error with Retry, unavailable and not found are shown truthfully', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/roles?mock=slow');
	await expect(page.getByRole('status').filter({ hasText: 'Loading roles' })).toBeAttached();
	await expect(rows(page)).toHaveCount(4);

	await visit(page, '/settings/roles?mock=fails');
	await expect(page.getByRole('alert')).toContainText("Couldn't load the roles");
	await expect(page.getByRole('alert')).toContainText("HQ didn't answer.");
	await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();

	await visit(page, '/settings/roles?mock=unavailable');
	await expect(page.locator('main')).toContainText("Roles isn't built yet");
	await expect(rows(page)).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'New role…' })).toHaveCount(0);

	await visit(page, '/settings/roles/no-such-role');
	await expect(heading(page)).toHaveText('Role not found');
	await expect(page).toHaveTitle('Role not found · Roles · Organization settings · Agentic Org');
	await expect(
		page.locator('main header').getByRole('link', { name: 'Roles', exact: true })
	).toBeVisible();
});

test('RL-8: unreachable data shows the last copy without changes; a viewer sees no actions', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/roles?mock=stale');
	await expect(page.getByTestId('stale')).toContainText(
		"Can't reach organization data. Showing the last copy from"
	);
	await expect(rows(page)).toHaveCount(4);
	// Changes are paused: the actions stay, disabled, and say why (RL-8).
	const paused = 'Changes are paused until organization data can be reached.';
	for (const name of ['New role…', 'Ask HR to draft…']) {
		const button = page.getByRole('button', { name });
		await expect(button).toBeDisabled();
		await expect(button).toHaveAccessibleDescription(paused);
	}
	await expect(page.getByRole('button', { name: 'Try again' })).toBeVisible();
	await visit(page, '/settings/roles/role-swe?mock=stale');
	await expect(page.getByTestId('stale')).toBeVisible();
	await expect(page.getByRole('button', { name: 'Edit' })).toBeDisabled();
	await expect(page.getByRole('button', { name: 'Edit' })).toHaveAccessibleDescription(paused);

	await visit(page, '/settings/roles?mock=viewer');
	await expect(rows(page)).toHaveCount(4);
	await expect(page.getByRole('button', { name: 'New role…' })).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'Ask HR to draft…' })).toHaveCount(0);
	await visit(page, '/settings/roles/role-swe?mock=viewer');
	await expect(heading(page)).toHaveText('Software Engineer');
	await expect(page.getByRole('button', { name: 'Edit' })).toHaveCount(0);
});

test('RL-8: a role file with details not recognized says which, and Fix rewrites it', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/roles?mock=invalid');
	const qa = rows(page).filter({ hasText: 'QA Engineer' });
	await expect(qa).toContainText('QA Engineer: details not recognized (key).');
	await qa.getByRole('button', { name: 'Fix QA Engineer' }).click();
	await expect(editor(page).getByRole('heading', { name: 'Fix QA Engineer' })).toBeVisible();
	await expect(page.getByTestId('invalid-note')).toContainText(
		'QA Engineer: details not recognized (key).'
	);
	await expect(page.getByLabel('Key (optional)')).toBeFocused();
	await page.getByLabel('Key (optional)').fill('qa-engineer');
	await editor(page).getByRole('button', { name: 'Save role' }).click();
	await expect(editor(page)).toBeHidden();
	await expect(qa).not.toContainText('details not recognized');
	await expect(qa).toContainText('qa-engineer');
	await expect(qa.getByRole('button', { name: 'Fix QA Engineer' })).toHaveCount(0);
	await expect(qa.getByRole('link', { name: 'QA Engineer' })).toBeFocused();
});

test('RL-10: at 320 px the list, the role page and the editor reflow with 44 px controls', async ({
	page
}) => {
	await page.setViewportSize({ width: 320, height: 720 });
	await openOrganization(page);
	await visit(page, '/settings/roles');
	await expect(rows(page)).toHaveCount(4);
	await noSidewaysScroll(page);
	const newRole = page.getByRole('button', { name: 'New role…' });
	expect((await newRole.boundingBox())!.height).toBeGreaterThanOrEqual(44);
	await expect(rows(page).nth(2)).toContainText('rev 3 · 2 employees');

	await visit(page, '/settings/roles/role-swe');
	await expect(heading(page)).toHaveText('Software Engineer');
	await noSidewaysScroll(page);
	const edit = page.getByRole('button', { name: 'Edit' });
	expect((await edit.boundingBox())!.height).toBeGreaterThanOrEqual(44);
	await edit.click();
	const sheet = editor(page);
	await expect(sheet).toBeVisible();
	expect((await sheet.boundingBox())!.width).toBeGreaterThanOrEqual(319);
	const save = sheet.getByRole('button', { name: 'Save role' });
	await save.scrollIntoViewIfNeeded();
	await expect(save).toBeInViewport();
	expect((await save.boundingBox())!.height).toBeGreaterThanOrEqual(44);
	await noSidewaysScroll(page);
});

// P2-1: at 320 px with 200% text the editor's footer never covers a field, the description column stays at least
// 200 px wide, and Save is reachable without being covered.
test('RL-10: at 320 px and 200% text the editor footer never covers the fields', async ({
	page
}) => {
	await page.setViewportSize({ width: 320, height: 800 });
	await openOrganization(page);
	await visit(page, '/settings/roles/role-swe');
	await page.evaluate(() => (document.documentElement.style.fontSize = '200%'));
	await page.getByRole('button', { name: 'Edit' }).click();
	const sheet = editor(page);
	const name = page.getByLabel('Name');
	await expect(name).toBeFocused();
	const uncovered = (id: string) =>
		page.evaluate((id) => {
			const el = document.getElementById(id)!;
			const r = el.getBoundingClientRect();
			const top = Math.max(r.top, 0) + 4;
			const bottom = Math.min(r.bottom, window.innerHeight) - 4;
			if (bottom <= top) return 'off screen';
			// Every visible horizontal band of the control is the control itself, not something over it.
			for (let y = top; y <= bottom; y += 8) {
				const hit = document.elementFromPoint(r.left + r.width / 2, y);
				if (!hit || !(hit === el || el.contains(hit) || hit.contains(el))) return `covered at ${y}`;
			}
			return 'ok';
		}, id);
	await expect.poll(() => uncovered('role-name')).toBe('ok');
	const description = sheet.getByText("A role describes an employee's work for teammates.");
	expect((await description.boundingBox())!.width).toBeGreaterThanOrEqual(200);
	const save = sheet.getByRole('button', { name: 'Save role' });
	await save.focus();
	await expect(save).toBeInViewport();
	expect(
		await save.evaluate((el) => {
			const r = el.getBoundingClientRect();
			const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
			return !!hit && (hit === el || el.contains(hit));
		})
	).toBe(true);
	await page.getByLabel('Key (optional)').focus();
	await expect.poll(() => uncovered('role-key')).toBe('ok');
	await noSidewaysScroll(page);
});

// P2-2: Back with the editor or the Ask HR dialog open closes it only (app-shell §6) and keeps what was typed.
test('RL-9: Back with the editor or Ask HR open closes it only and keeps the typed text', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/roles');
	await page.getByRole('link', { name: 'Staff Engineer' }).click();
	await expect(heading(page)).toHaveText('Staff Engineer');
	const edit = page.getByRole('button', { name: 'Edit' });
	await edit.click();
	await page.getByLabel('Name').fill('Principal Engineer');
	await page.goBack();
	await expect(editor(page)).toBeHidden();
	await expect(page).toHaveURL(/\/settings\/roles\/role-staff$/);
	await expect(edit).toBeFocused();
	await edit.click();
	await expect(page.getByLabel('Name')).toHaveValue('Principal Engineer');
	await expect(page.getByTestId('restored-note')).toHaveText(
		'Your unsaved changes are kept. Cancel discards them.'
	);
	// Cancel is the deliberate discard: the next opening starts from the role again.
	await editor(page).getByRole('button', { name: 'Cancel' }).click();
	await edit.click();
	await expect(page.getByLabel('Name')).toHaveValue('Staff Engineer');
	await expect(page.getByTestId('restored-note')).toHaveCount(0);
	await editor(page).getByRole('button', { name: 'Cancel' }).click();

	await page.locator('main header').getByRole('link', { name: 'Roles', exact: true }).click();
	const ask = page.getByRole('button', { name: 'Ask HR to draft…' });
	await ask.click();
	await page.getByLabel('What should this role do?').fill('Keep releases moving');
	await page.goBack();
	await expect(page.getByRole('dialog')).toBeHidden();
	await expect(page).toHaveURL(/\/settings\/roles$/);
	await expect(ask).toBeFocused();
	await ask.click();
	await expect(page.getByLabel('What should this role do?')).toHaveValue('Keep releases moving');
});

// P2-3 and P2-4: with no roles the view has one primary action, and the empty copy only invites what the viewer
// can do now.
test('RL-8: the empty state has one primary action and never invites what the viewer cannot do', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/roles?mock=empty');
	await expect(page.getByRole('heading', { name: 'No roles yet' })).toBeVisible();
	await expect(page.getByRole('button', { name: 'New role…' })).toHaveCount(1);
	await expect(page.locator('main header').getByRole('button')).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'Ask HR to draft…' })).toHaveCount(1);
	await expect(page.getByRole('button', { name: 'Ask HR to draft…' })).toBeEnabled();

	const invite = /Create one/;
	const none = "Roles describe employees' work. None have been created yet.";
	await visit(page, '/settings/roles?mock=empty,viewer');
	await expect(page.getByRole('heading', { name: 'No roles yet' })).toBeVisible();
	await expect(page.locator('main')).toContainText(none);
	await expect(page.locator('main')).not.toContainText(invite);
	await expect(page.getByRole('button', { name: 'New role…' })).toHaveCount(0);

	await visit(page, '/settings/roles?mock=empty,stale');
	await expect(page.getByRole('heading', { name: 'No roles yet' })).toBeVisible();
	await expect(page.locator('main')).toContainText(none);
	await expect(page.locator('main')).not.toContainText(invite);
	await expect(page.getByRole('button', { name: 'New role…' })).toBeDisabled();
});

// P3-2: a failed draft's message is not shown again when the dialog reopens; the typed text is.
test('RL-3: reopening Ask HR after a failure keeps the text and clears the old refusal', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/roles?mock=save-fails');
	const ask = page.getByRole('button', { name: 'Ask HR to draft…' });
	await ask.click();
	const dialog = page.getByRole('dialog');
	await page.getByLabel('What should this role do?').fill('Keep releases moving');
	await dialog.getByRole('button', { name: 'Ask HR' }).click();
	await expect(dialog.getByRole('alert')).toContainText('The HR lead stopped');
	await dialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(ask).toBeFocused();
	await ask.click();
	await expect(page.getByLabel('What should this role do?')).toHaveValue('Keep releases moving');
	await expect(dialog.getByRole('alert')).toHaveCount(0);
});

test('SIGN-19: cross-tab sign-out asks before discarding open or retained role drafts, honors Stay, and closes/clears on confirm', async ({
	page,
	context
}) => {
	await openOrganization(page);
	await visit(page, '/settings/roles');
	await page.getByRole('button', { name: 'New role…' }).click();
	await expect(editor(page)).toBeVisible();
	await editor(page).getByLabel('Name').fill('Confidential Role');
	await editor(page).getByLabel('Text').fill('Secret draft body');

	// Sign out from a second tab via the real account menu: SIGN-19 asks before discarding Tab 1's draft.
	const otherTab = await context.newPage();
	await visit(otherTab, '/settings/roles');
	await otherTab.getByRole('button', { name: 'You · Owner' }).click();
	await otherTab.getByRole('menuitem', { name: 'Sign out', exact: true }).click();
	const confirmHeading = otherTab.getByRole('heading', { name: /Sign out\?/ });
	await expect(confirmHeading).toBeVisible();

	// Choosing Stay keeps Tab 2 signed in and leaves Tab 1's open draft intact.
	await otherTab.getByRole('button', { name: 'Stay', exact: true }).click();
	await expect(confirmHeading).toBeHidden();
	await expect(otherTab).toHaveURL(/\/settings\/roles$/);
	await expect(editor(page)).toBeVisible();
	await expect(editor(page).getByLabel('Name')).toHaveValue('Confidential Role');

	// Closing Tab 1 without Cancel retains the draft; cross-tab sign-out must still confirm first.
	await editor(page).getByRole('button', { name: 'Close' }).click();
	await expect(editor(page)).toBeHidden();
	await expect(page).toHaveURL(/\/settings\/roles$/);
	await otherTab.getByRole('button', { name: 'You · Owner' }).click();
	await otherTab.getByRole('menuitem', { name: 'Sign out', exact: true }).click();
	await expect(confirmHeading).toBeVisible();
	await otherTab.getByRole('button', { name: 'Stay', exact: true }).click();
	await expect(confirmHeading).toBeHidden();

	// Reopen Tab 1 (restores retained draft), then confirm Sign out from Tab 2.
	await page.getByRole('button', { name: 'New role…' }).click();
	await expect(editor(page).getByLabel('Name')).toHaveValue('Confidential Role');
	await otherTab.getByRole('button', { name: 'You · Owner' }).click();
	await otherTab.getByRole('menuitem', { name: 'Sign out', exact: true }).click();
	await expect(confirmHeading).toBeVisible();
	await otherTab.getByRole('button', { name: 'Sign out', exact: true }).click();
	await expect(otherTab).toHaveURL(/\/sign-in/);

	// SIGN-20: Tab 1 must also leave Roles to /sign-in and clear its open draft without leaking data.
	await expect(page).toHaveURL(/\/sign-in/);
	await expect(editor(page)).toBeHidden();
	await expect(page.locator('body')).not.toContainText('Confidential Role');
	await expect(page.locator('body')).not.toContainText('Secret draft body');
	await otherTab.close();
});

test('SIGN-20: overlapping sign-out confirmations dismiss on the other tab once one tab confirms', async ({
	page,
	context
}) => {
	await openOrganization(page);
	await visit(page, '/settings/roles');
	await page.getByRole('button', { name: 'New role…' }).click();
	await editor(page).getByLabel('Name').fill('Overlap Draft');
	await editor(page).getByRole('button', { name: 'Close' }).click();
	await expect(editor(page)).toBeHidden();

	const otherTab = await context.newPage();
	await visit(otherTab, '/settings/roles');

	// Open the sign-out confirmation dialog in BOTH tabs simultaneously.
	await page.getByRole('button', { name: 'You · Owner' }).click();
	await page.getByRole('menuitem', { name: 'Sign out', exact: true }).click();
	const tab1Confirm = page.getByRole('heading', { name: /Sign out\?/ });
	await expect(tab1Confirm).toBeVisible();

	await otherTab.getByRole('button', { name: 'You · Owner' }).click();
	await otherTab.getByRole('menuitem', { name: 'Sign out', exact: true }).click();
	const tab2Confirm = otherTab.getByRole('heading', { name: /Sign out\?/ });
	await expect(tab2Confirm).toBeVisible();

	// Confirm in Tab 2: Tab 1 must navigate to /sign-in with its confirmation modal dismissed.
	await otherTab.getByRole('button', { name: 'Sign out', exact: true }).click();
	await expect(otherTab).toHaveURL(/\/sign-in/);
	await expect(page).toHaveURL(/\/sign-in/);
	await expect(tab1Confirm).toBeHidden();
	await expect(page.getByLabel('Username')).toBeVisible();
	await otherTab.close();
});
