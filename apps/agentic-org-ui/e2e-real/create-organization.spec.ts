import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { startInstance, type Instance } from './instance';
import { removeInstance } from './teardown';

// A4.5: create organization in the real browser against a real host, web process and worker
// (FC-018; offices-and-teams UX §5a). Its own instance under its own disposable root and port
// range: the setup link works once, and first-slice.spec.ts uses and then stops the shared one.
const shots = '../../.agent-kit/logs/a4/screens';
const PREFIX = 'a4pw';
const PORT_BASE = 24180;
const login = 'alex';
const password = 'correct horse battery';
const organization = { name: 'Side Project', slug: 'side-project' };
// A syntactically valid remote; nothing is fetched or pushed at create.
const repository = 'https://example.test/org/state.git';

let instance: Instance | undefined;

test.beforeAll(async () => {
	instance = await startInstance(PREFIX, PORT_BASE);
});
test.afterAll(() => {
	removeInstance(instance?.root, PREFIX);
});

async function axe(page: Page) {
	const result = await new AxeBuilder({ page }).analyze();
	expect(
		result.violations.map(
			(v) => `${v.id}: ${v.help} @ ${v.nodes.map((n) => n.target.join(' ')).join('; ')}`
		)
	).toEqual([]);
}

test('setup link → register → create with all four fields → recovery command → listed, marked, kept on reload', async ({
	page
}) => {
	const { link, base } = instance!;
	const requests: string[] = [];
	page.on('request', (request) => {
		const url = new URL(request.url());
		if (url.pathname.startsWith('/api/v1/organizations'))
			requests.push(`${request.method()} ${url.pathname}`);
	});

	await page.goto(link);
	await page.getByLabel('Username').fill(login);
	await page.getByLabel('Password (at least 12 characters)').fill(password);
	await page.getByLabel('Password again').fill(password);
	await page.getByRole('button', { name: /Create account|Register/ }).click();
	await expect(page).toHaveURL(/\/welcome$/);
	await expect(
		page.getByRole('heading', { name: 'No organizations yet. Create one to get started.' })
	).toBeVisible();

	await page.getByRole('link', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/create$/);
	await expect(page.getByRole('heading', { level: 1, name: 'Create organization' })).toBeVisible();
	// The real instance names itself (GET /api/v1/instance).
	await expect(page.locator('#create-note')).toContainText(
		'It will be stored on HQ instance default'
	);
	await page.getByLabel('Name', { exact: true }).fill(organization.name);
	await expect(page.getByLabel('Slug', { exact: true })).toHaveValue(organization.slug);
	await page.getByLabel('Slug', { exact: true }).fill(organization.slug);
	await page.getByLabel('Org Git URL').fill(repository);
	await page.getByLabel('State branch').fill('main');
	await axe(page);
	await page.screenshot({ path: `${shots}/real-01-create-form.png` });

	await page.getByRole('button', { name: 'Create organization', exact: true }).click();
	// The real create starts a worker: the page says so while it runs.
	await expect(page.getByRole('button', { name: 'Creating…' })).toBeDisabled();
	await page.screenshot({ path: `${shots}/real-02-creating.png` });

	await expect(
		page.getByRole('heading', { level: 1, name: `Created ${organization.name}. You own it.` })
	).toBeVisible({
		timeout: 120_000
	});
	await expect(page.getByTestId('recovery-command')).toHaveText(
		`agentic-org recovery release --org '${organization.name}'`
	);
	await expect(page.getByText('HQ instance default')).toBeVisible();
	// The recovery code itself never reaches the browser.
	await expect(
		page.getByRole('heading', { name: 'Recovery code: waiting to be collected' })
	).toBeVisible();
	await axe(page);
	await page.screenshot({ path: `${shots}/real-03-created.png` });
	expect(requests.filter((r) => r === 'POST /api/v1/organizations')).toHaveLength(1);

	// The organizations page lists it with its HQ office and marks it as the one this browser has open.
	await page.goto(`${base}/welcome`);
	const row = page.getByTestId('organization-row');
	await expect(row).toHaveCount(1);
	await expect(row).toContainText(organization.name);
	await expect(row).toContainText('HQ · ');
	await expect(row.getByText('Open now')).toBeVisible();
	await expect(row.getByText('Unavailable')).toHaveCount(0);
	await expect(page.getByRole('button', { name: `Open ${organization.name}` })).toBeEnabled();
	await expect(page.getByTestId('organization-creating')).toHaveCount(0);
	await axe(page);
	await page.screenshot({ path: `${shots}/real-04-listed.png` });

	// A reload keeps the session, the organization and the mark (stored at the host; ids only in the browser).
	await page.reload();
	await expect(row).toHaveCount(1);
	await expect(row).toContainText(organization.name);
	await expect(row).toContainText('HQ · ');
	await expect(row.getByText('Open now')).toBeVisible();
	await page.screenshot({ path: `${shots}/real-05-reloaded.png` });
});
