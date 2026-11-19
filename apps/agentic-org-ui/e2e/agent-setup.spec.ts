// Organization settings › Agent setup against the dev mock (our-org-in-the-app §5 and §9, AC-IA07-1–3; agent-run-modes
// §2; app-shell-ux §2, §4, §6a). The mock's switches on the address select the states: ?mock=slow|fails|empty|
// unavailable|not-found|viewer|conflict|save-fails (e2e/fakes/modules/agent-setup.ts).
import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { signInAsAdmin } from './web-session';

async function openOrganization(page: Page) {
	await signInAsAdmin(page, [
		{
			id: 'org-agent-setup',
			name: 'Lumen Labs',
			created_at: '2026-11-19T00:00:00Z',
			recovery: 'acknowledged'
		}
	]);
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Lumen Labs');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).not.toHaveURL(/\/create$/);
}

const rows = (page: Page) => page.locator('main ul[aria-labelledby="agent-setup-heading"] > li');
const row = (page: Page, id: string) => page.locator(`main li[data-agent="${id}"]`);
const heading = (page: Page) => page.locator('#agent-heading');
const health = (page: Page) => page.getByTestId('health');

async function noSidewaysScroll(page: Page) {
	const overflow = await page.evaluate(
		() => document.documentElement.scrollWidth - document.documentElement.clientWidth
	);
	expect(overflow).toBeLessThanOrEqual(0);
}

test('AS-1, AS-2: the list shows each agent with runtime, model, profile, office and one Health word', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/agents');
	await expect(page).toHaveTitle('Agent setup · Organization settings · Agentic Org');
	await expect(page.getByRole('heading', { level: 2, name: /^Agent setup/ })).toContainText('9');
	await expect(rows(page)).toHaveCount(9);
	await expect(page.getByText(/4 of 9 need setup\./)).toBeVisible();

	await expect(row(page, 'noor')).toContainText('Claude Code · claude-sonnet-5');
	await expect(row(page, 'noor')).toContainText('Primary pool');
	await expect(row(page, 'noor')).toContainText('HQ');
	await expect(row(page, 'noor')).toContainText('Health: Ready');
	// Readiness the office can't report is said, never read as ready.
	await expect(row(page, 'mira')).toContainText('Health: Runtime readiness unknown');
	await expect(row(page, 'pax')).toContainText('Execution not configured');
	await expect(row(page, 'pax')).toContainText('Health: Needs setup: Execution not configured');
	await expect(row(page, 'ines')).toContainText('Unplaced');
	await expect(row(page, 'ines')).toContainText('Health: Needs setup: Unplaced');
	// Quinn's record has no home team: an invalid record, never "No team".
	await expect(row(page, 'quinn')).toContainText(
		'Health: Needs setup: Home team missing: this record is invalid (and 1 more)'
	);
	// The strict missing-model predicate: Kai's exact model is absent from the reported list.
	await expect(row(page, 'kai')).toContainText('(and 1 more)');

	await expect(page.getByRole('link', { name: 'Hire an agent' })).toHaveAttribute(
		'href',
		/\/teams\/hire$/
	);
});

test('AS-10: the built-in HR lead is listed first as Built-in, and can be paused but not offboarded', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/agents');
	// Created with the organization (our-org-in-the-app §2a): always in the list, before the hired agents.
	await expect(rows(page).first()).toContainText('HR lead');
	await expect(rows(page).first()).toContainText('@hr');
	await expect(rows(page).first()).toContainText('Built-in');
	await rows(page).first().getByRole('link', { name: 'HR lead' }).click();
	await expect(heading(page)).toHaveText('HR lead');
	await expect(page.getByText('Agent, built in')).toBeVisible();
	await page.getByRole('button', { name: /^More/ }).click();
	await expect(page.getByRole('menuitem', { name: 'Pause…' })).toBeVisible();
	await expect(page.getByRole('menuitem', { name: 'Offboard…' })).toHaveCount(0);
});

test(
	'AS-4, AS-12: an agent page shows health with fixes, how and where it runs, and its record',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOrganization(page);
		await visit(page, '/settings/agents');
		await page.getByRole('link', { name: 'Kai Lindqvist' }).click();
		await expect(page).toHaveURL(/\/settings\/agents\/kai$/);
		await expect(heading(page)).toHaveText('Kai Lindqvist');
		await expect(heading(page)).toBeFocused();
		await expect(page).toHaveTitle(
			'Kai Lindqvist · Agent setup · Organization settings · Agentic Org'
		);
		await expect(health(page)).toContainText("Codex isn't signed in on HQ (This Mac)");
		await expect(health(page)).toContainText('codex login');
		await expect(health(page)).toContainText("gpt-5.1-codex isn't offered by Codex on HQ");
		await expect(page.getByText('Execution configuration, rev 4')).toBeVisible();
		await expect(page.getByText('1. OpenAI · subscription C')).toBeVisible();
		await expect(page.getByText('Not signed in', { exact: true })).toBeVisible();
		await expect(page.getByText('HQ · placement generation 1')).toBeVisible();
		await expect(page.getByText('/srv/agentic-org/lumen/agents/kai')).toBeVisible();
		await expect(page.getByRole('button', { name: 'Copy path' })).toBeVisible();
		await expect(
			page.getByRole('region', { name: 'Employee record' }).getByText('Software Engineer')
		).toBeVisible();

		await page.getByRole('link', { name: 'View profile' }).click();
		await expect(page).toHaveURL(/\/members\/kai$/);
		// The profile's Open Agent setup comes back to this agent's setup.
		await page.getByRole('link', { name: 'Open Agent setup' }).click();
		await expect(page).toHaveURL(/\/settings\/agents\/kai$/);
		await expect(heading(page)).toHaveText('Kai Lindqvist');
	}
);

test('AS-5: Edit saves the profile and model; a new profile never keeps a model it does not offer', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/agents/kai');
	await page.getByRole('button', { name: 'Edit', exact: true }).click();
	const dialog = page.getByRole('dialog');
	await expect(dialog.getByRole('heading', { name: 'Edit how Kai Lindqvist runs' })).toBeVisible();
	await dialog.getByRole('radio', { name: /Primary pool/ }).check();
	await expect(dialog.getByRole('radio', { name: /claude-sonnet-5/ })).not.toBeChecked();
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(dialog.getByText('Choose a model to save.')).toBeVisible();
	// The refusal focuses the field it is about (app-shell-ux §6a).
	await expect(dialog.getByRole('radio', { name: /claude-sonnet-5/ })).toBeFocused();
	await dialog.getByRole('radio', { name: /claude-opus-5/ }).check();
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(dialog).toBeHidden();
	await expect(
		page.getByText(/runs claude-opus-5 on Primary pool from its next turn/)
	).toBeVisible();
	await expect(page.getByText('Execution configuration, rev 5')).toBeVisible();
	await expect(page.getByRole('button', { name: 'Edit', exact: true })).toBeFocused();
	await expect(health(page)).toContainText('Ready');

	await visit(page, '/settings/agents/pax');
	await page.getByRole('button', { name: 'Set up execution' }).click();
	await dialog.getByRole('radio', { name: /Anthropic API/ }).check();
	await dialog.getByLabel('Model').fill('claude-haiku-5');
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByText('claude-haiku-5', { exact: true })).toBeVisible();
});

test('AS-5: a conflicting save shows the newer revision and keeps the choice; a refusal stays inline', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/agents/noor?mock=conflict');
	await page.getByRole('button', { name: 'Edit', exact: true }).click();
	const dialog = page.getByRole('dialog');
	await dialog.getByRole('radio', { name: /claude-opus-5/ }).check();
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(dialog.getByRole('alert')).toContainText('changed while you were editing');
	await expect(dialog.getByRole('alert')).toBeFocused();
	await expect(dialog.getByRole('radio', { name: /claude-opus-5/ })).toBeChecked();
	await expect(dialog.getByTestId('now-saved')).toHaveText(
		'Saved now, rev 3 by Sam Okafor: Primary pool · claude-sonnet-5.'
	);
	await expect(
		page.getByText(/Execution configuration, rev 3 · updated by Sam Okafor/)
	).toBeAttached();
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByText(/Execution configuration, rev 4/)).toBeVisible();

	await visit(page, '/settings/agents/noor?mock=save-fails');
	await page.getByRole('button', { name: 'Edit', exact: true }).click();
	await dialog.getByRole('radio', { name: /claude-opus-5/ }).check();
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(dialog.getByRole('alert')).toContainText("HQ couldn't write the agent's files");
	await expect(dialog.getByRole('alert')).toBeFocused();
	await page.keyboard.press('Tab');
	await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeFocused();
	await expect(dialog).toBeVisible();
});

test('AS-3, AS-5: a turned-off execution configuration needs setup, and nothing promises a save turns it on', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/agents/noor?mock=disabled');
	await expect(health(page)).toContainText('Execution is turned off in its configuration');
	await expect(health(page)).toContainText("Turning it on isn't available here yet.");
	await expect(page.getByText('Turned off in its configuration', { exact: true })).toBeVisible();
	await page.getByRole('button', { name: 'Edit', exact: true }).click();
	const dialog = page.getByRole('dialog');
	await expect(dialog.getByRole('heading', { name: 'Edit how Noor Haddad runs' })).toBeVisible();
	await expect(dialog).not.toContainText(/back on|turns? (it|its execution) on/i);
	// Saving the same profile and model is no change: the dialog closes without a write.
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByText('Execution configuration, rev 2')).toBeVisible();
	await expect(health(page)).toContainText('Execution is turned off in its configuration');
});

test('AS-6: Change role writes the role only; no role reads No role on record', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/agents/quinn');
	await expect(page.getByText('No role on record')).toBeVisible();
	await expect(page.getByText('Role as written: researcher')).toBeVisible();
	await page.getByRole('button', { name: 'Change role…' }).click();
	const dialog = page.getByRole('dialog');
	await dialog.getByLabel('Role').selectOption({ label: 'QA Engineer' });
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByText("Quinn Adeyemi's role is QA Engineer.")).toBeVisible();
	await expect(page.getByText(/Who Quinn Adeyemi is, rev 2/)).toBeVisible();
	await expect(page.getByText('Execution configuration, rev 2')).toBeVisible();
});

test('AS-6: a refused role change keeps the dialog and focuses its reason', async ({ page }) => {
	await openOrganization(page);
	await visit(page, '/settings/agents/noor?mock=save-fails');
	await page.getByRole('button', { name: 'Change role…' }).click();
	const dialog = page.getByRole('dialog');
	await dialog.getByLabel('Role').selectOption({ label: 'QA Engineer' });
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(dialog.getByRole('alert')).toContainText("HQ couldn't write the agent's files");
	await expect(dialog.getByRole('alert')).toBeFocused();
	await expect(dialog.getByLabel('Role')).toHaveValue('role-qa');
});

test('AS-7: Pause names the effect, shows Paused and stop pending; Resume; Offboard previews and leaves', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/agents/juno');
	await page.getByRole('button', { name: /^More/ }).click();
	await page.getByRole('menuitem', { name: 'Pause…' }).click();
	const dialog = page.getByRole('dialog');
	await expect(dialog).toContainText('HQ will record the request. Juno Park stays in');
	await dialog.getByRole('button', { name: 'Pause' }).click();
	await expect(dialog).toBeHidden();
	await expect(health(page)).toContainText('Paused');
	await expect(page.getByTestId('stop-pending')).toBeVisible();
	await expect(page.getByRole('button', { name: /^More/ })).toBeFocused();

	await page.getByRole('button', { name: /^More/ }).click();
	await page.getByRole('menuitem', { name: 'Resume' }).click();
	await expect(page.getByText('Resumed Juno Park.')).toBeVisible();
	await expect(health(page)).toContainText('Ready');

	await page.getByRole('button', { name: /^More/ }).click();
	await page.getByRole('menuitem', { name: 'Offboard…' }).click();
	await expect(dialog).toContainText(
		"Juno Park's profile and home-team history stay in organization data. The handle stays reserved."
	);
	await dialog.getByRole('button', { name: 'Offboard' }).click();
	await expect(page).toHaveURL(/\/settings\/agents$/);
	await expect(rows(page)).toHaveCount(8);
	await expect(row(page, 'juno')).toHaveCount(0);
});

test('AS-11: loading, error with Retry, empty, unavailable, not found and a read-only viewer', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/agents?mock=slow');
	await expect(page.getByRole('status').filter({ hasText: /loading agent setup/i })).toBeAttached();

	await visit(page, '/settings/agents?mock=fails');
	await expect(page.getByRole('heading', { name: "Couldn't load agent setup" })).toBeVisible();
	await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();

	await visit(page, '/settings/agents?mock=empty');
	await expect(page.getByRole('heading', { name: 'No agents yet' })).toBeVisible();
	await expect(page.getByRole('link', { name: 'Hire an agent' })).toHaveCount(2);

	await visit(page, '/settings/agents?mock=unavailable');
	await expect(
		page.getByText(/isn't built yet|Not in this release|not available/i).first()
	).toBeVisible();
	await expect(rows(page)).toHaveCount(0);

	await visit(page, '/settings/agents/nobody');
	await expect(heading(page)).toHaveText('Agent not found');
	await expect(page).toHaveTitle(
		'Agent not found · Agent setup · Organization settings · Agentic Org'
	);
	await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0);

	await visit(page, '/settings/agents/noor?mock=viewer');
	await expect(heading(page)).toHaveText('Noor Haddad');
	await expect(page.getByRole('button', { name: 'Edit', exact: true })).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'Change role…' })).toHaveCount(0);
	await expect(page.getByRole('button', { name: /^More/ })).toHaveCount(0);
	await expect(page.getByRole('link', { name: 'View profile' })).toBeVisible();
});

/**
 * The whole row is the agent's link (the stretched title link): a tap anywhere on the row, away from the name, lands
 * on that link. Measured where the name isn't, so it fails if the row-wide target is missing.
 */
async function rowIsTarget(page: Page, id: string) {
	const target = await row(page, id).evaluate((li) => {
		li.scrollIntoView({ block: 'center' });
		const box = li.getBoundingClientRect();
		const link = li.querySelector('a')!;
		const hit = document.elementFromPoint(box.right - 8, box.bottom - 8);
		return { height: box.height, onLink: !!hit && (hit === link || link.contains(hit)) };
	});
	expect(target.height).toBeGreaterThanOrEqual(44);
	expect(target.onLink, `${id}'s row is its link's target`).toBe(true);
}

async function targetsAtLeast44(page: Page, controls: [role: 'link' | 'button', name: string][]) {
	for (const [role, name] of controls) {
		const box = await page.getByRole(role, { name, exact: true }).boundingBox();
		expect(box, name).not.toBeNull();
		expect(box!.height, name).toBeGreaterThanOrEqual(44);
	}
}

const agentPageControls: [role: 'link' | 'button', name: string][] = [
	['link', 'View profile'],
	['button', 'Edit'],
	['button', 'Change role…'],
	['button', 'Copy path']
];

test('AS-13: 320 px and 200% text reflow without sideways scroll, with 44 px targets', async ({
	page
}) => {
	await openOrganization(page);
	await page.setViewportSize({ width: 320, height: 800 });
	await visit(page, '/settings/agents');
	await expect(rows(page)).toHaveCount(9);
	await noSidewaysScroll(page);
	await rowIsTarget(page, 'kai');
	await targetsAtLeast44(page, [['link', 'Hire an agent']]);
	// The Health pill keeps whole words.
	const pill = row(page, 'mira').locator('[data-health]');
	expect(await pill.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);

	await visit(page, '/settings/agents/kai');
	await expect(heading(page)).toHaveText('Kai Lindqvist');
	await noSidewaysScroll(page);
	await targetsAtLeast44(page, agentPageControls);

	// 200% text: the same pages, again without sideways scroll, with the same targets and whole-word pills.
	const largeText = () => page.evaluate(() => (document.documentElement.style.fontSize = '200%'));
	await largeText();
	await expect(heading(page)).toHaveText('Kai Lindqvist');
	await noSidewaysScroll(page);
	await targetsAtLeast44(page, agentPageControls);
	const badge = page.getByTestId('health').locator('[data-health]');
	expect(await badge.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);

	await visit(page, '/settings/agents');
	await expect(rows(page)).toHaveCount(9);
	await largeText();
	await noSidewaysScroll(page);
	await rowIsTarget(page, 'kai');
	await targetsAtLeast44(page, [['link', 'Hire an agent']]);
	expect(await pill.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
});
