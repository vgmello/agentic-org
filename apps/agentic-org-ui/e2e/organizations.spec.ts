import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { organizationView, signInAsAdmin, type WebOrganization } from './web-session';

// Create organization against the mocked web API (fakes/session-server.ts): offices-and-teams
// UX §5a, OFC-15 to OFC-24, and the create items of FC-018. The spec's copy is the requirement.
const REPOSITORY = 'https://example.test/org/state.git';
type Stats = {
	creates: { operation_id: string; params: Record<string, unknown> }[];
	lookups: string[];
};

async function backend(page: Page) {
	const context = page.context();
	const id = (await context.cookies()).find((cookie) => cookie.name === 'p6_auth_case')!.value;
	return {
		async set(action: 'organizations' | 'creating' | 'listUnavailable', value: unknown) {
			const reply = await context.request.post('/__p6-auth/control', {
				data: { id, action, value }
			});
			expect(reply.ok()).toBe(true);
		},
		async script(action: 'createScript' | 'lookupScript', value: unknown[]) {
			const reply = await context.request.post('/__p6-auth/control', {
				data: { id, action, value }
			});
			expect(reply.ok()).toBe(true);
		},
		async stats(): Promise<Stats> {
			const reply = await context.request.get(`/__p6-auth/control?id=${id}`);
			expect(reply.ok()).toBe(true);
			return reply.json();
		}
	};
}
async function open(page: Page) {
	await signInAsAdmin(page);
	const server = await backend(page);
	await page.getByRole('link', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/create$/);
	await expect(page.getByText('HQ instance default')).toBeVisible();
	return server;
}
async function fill(page: Page, name = 'Side Project') {
	await page.getByLabel('Name', { exact: true }).fill(name);
	await page.getByLabel('Org Git URL').fill(REPOSITORY);
	await page.getByLabel('State branch').fill('main');
}
const create = (page: Page) =>
	page.getByRole('button', { name: 'Create organization', exact: true });
const axe = async (page: Page) =>
	(await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze())
		.violations;

test('the form asks for name, slug, Org Git URL and state branch, names the HQ instance, and is accessible (OFC-15, OFC-24)', async ({
	page
}) => {
	await open(page);
	await expect(page.getByRole('heading', { level: 1, name: 'Create organization' })).toBeVisible();
	await expect(page.getByRole('textbox')).toHaveCount(4);
	for (const label of ['Name', 'Slug', 'Org Git URL', 'State branch'])
		await expect(page.getByLabel(label, { exact: true })).toBeEditable();
	await expect(page.locator('input[type=file]')).toHaveCount(0);
	await expect(page.getByText(/team/i)).toHaveCount(0);
	await expect(page.locator('#create-note')).toHaveText(
		"It will be stored on HQ instance default, and you'll own it. Its recovery code is then collected on that machine."
	);
	const described = await page.getByLabel('Name', { exact: true }).getAttribute('aria-describedby');
	expect(described).toContain('create-note');
	expect(await axe(page)).toEqual([]);
});

test('the slug follows the name until it is edited, and is sent as typed', async ({ page }) => {
	const server = await open(page);
	const slug = page.getByLabel('Slug', { exact: true });
	await page.getByLabel('Name', { exact: true }).fill('Café Zürich & Co');
	await expect(slug).toHaveValue('cafe-zurich-co');
	await slug.fill('cz');
	await page.getByLabel('Name', { exact: true }).fill('  Café Zürich  ');
	await expect(slug).toHaveValue('cz');
	await page.getByLabel('Org Git URL').fill(REPOSITORY);
	await page.getByLabel('State branch').fill('org/state');
	await server.script('createScript', ['completed']);
	await create(page).click();
	await expect(
		page.getByRole('heading', { name: 'Created Café Zürich. You own it.' })
	).toBeFocused();
	expect((await server.stats()).creates.map((c) => c.params)).toEqual([
		{
			name: 'Café Zürich',
			slug: 'cz',
			organization_repository: { kind: 'remote', remote_url: REPOSITORY },
			state_branch: 'org/state'
		}
	]);
});

test('the slug can be emptied and typed again; left empty, it follows the name again', async ({
	page
}) => {
	await open(page);
	const name = page.getByLabel('Name', { exact: true });
	const slug = page.getByLabel('Slug', { exact: true });
	await name.fill('Side Project');
	await expect(slug).toHaveValue('side-project');
	await slug.focus();
	await slug.press('ControlOrMeta+a');
	await slug.press('Backspace');
	await expect(slug).toHaveValue('');
	await expect(slug).toBeFocused();
	await slug.pressSequentially('sp');
	await expect(slug).toHaveValue('sp');
	await slug.press('ControlOrMeta+a');
	await slug.press('Backspace');
	await expect(slug).toHaveValue('');
	await name.focus();
	await expect(slug).toHaveValue('side-project');
	await name.fill('Other');
	await expect(slug).toHaveValue('other');
	// Leaving a slug the form refused, without changing it, keeps its message.
	await slug.fill('Not Kebab');
	await create(page).click();
	const rule = page.getByText(
		'Use lowercase letters, numbers and single hyphens, for example side-project.'
	);
	await expect(rule).toBeVisible();
	await expect(slug).toBeFocused();
	await slug.press('Tab');
	await expect(slug).not.toBeFocused();
	await expect(rule).toBeVisible();
	await expect(slug).toHaveValue('Not Kebab');
});

test("a name HQ refuses as invalid shows the inline message for HQ's rule", async ({ page }) => {
	const server = await open(page);
	await fill(page);
	await server.script('createScript', [
		{ refused: { field: 'name', rule: 'too_long' } },
		{ refused: { field: 'name', rule: 'format' } }
	]);
	await create(page).click();
	await expect(page.getByText('Use 64 characters or fewer.')).toBeVisible();
	await expect(page.getByLabel('Name', { exact: true })).toBeFocused();
	await create(page).click();
	await expect(
		page.getByText("HQ didn't accept this name. Change it and try again.")
	).toBeVisible();
	expect((await server.stats()).creates).toEqual([]);
});

test(
	'an invalid name or an empty field shows its message, sends nothing, and clears on typing (OFC-16, FC-018 (6))',
	{ tag: '@webkit' },
	async ({ page }) => {
		const server = await open(page);
		const name = page.getByLabel('Name', { exact: true });
		await create(page).click();
		await expect(page.getByText('Give the organization a name.')).toBeVisible();
		await expect(page.getByText('Give the organization a slug.')).toBeVisible();
		await expect(page.getByText("Enter the remote repository's address.")).toBeVisible();
		await expect(page.getByText('Enter the state branch.')).toBeVisible();
		await expect(name).toHaveAttribute('aria-invalid', 'true');
		await expect(name).toBeFocused();
		expect(await name.getAttribute('aria-describedby')).toContain('name-error');
		expect(await axe(page)).toEqual([]);
		await name.fill('x'.repeat(65));
		await expect(page.getByText('Give the organization a name.')).toHaveCount(0);
		await expect(name).not.toHaveAttribute('aria-invalid', 'true');
		await expect(page.getByText('Enter the state branch.')).toBeVisible();
		await create(page).click();
		await expect(page.getByText('Use 64 characters or fewer.')).toBeVisible();
		await fill(page, 'bad‮name');
		await create(page).click();
		await expect(
			page.getByText("The name can't contain invisible or direction-changing characters.")
		).toBeVisible();
		await page.getByLabel('Org Git URL').fill('/Users/alex/org');
		await fill(page, 'Fine');
		await page.getByLabel('Org Git URL').fill('/Users/alex/org');
		await create(page).click();
		await expect(page.getByText(/Org Git must be a remote repository/)).toBeVisible();
		expect((await server.stats()).creates).toEqual([]);
		await expect(create(page)).toBeEnabled();
	}
);

test('typing in a field while other fields still show errors keeps focus and every character in that field', async ({
	page
}) => {
	const server = await open(page);
	const name = page.getByLabel('Name', { exact: true });
	const slug = page.getByLabel('Slug', { exact: true });
	const repository = page.getByLabel('Org Git URL');
	const branch = page.getByLabel('State branch');

	// Only Name filled, then Create: the repository and the branch are both missing.
	await name.fill('Side Project');
	await create(page).click();
	await expect(page.getByText("Enter the remote repository's address.")).toBeVisible();
	await expect(page.getByText('Enter the state branch.')).toBeVisible();
	await expect(repository).toBeFocused();
	await repository.pressSequentially(REPOSITORY);
	await expect(repository).toBeFocused();
	await expect(repository).toHaveValue(REPOSITORY);
	await expect(branch).toHaveValue('');
	await expect(page.getByText("Enter the remote repository's address.")).toHaveCount(0);
	await expect(page.getByText('Enter the state branch.')).toBeVisible();

	// An empty form, then typing in Name: three other fields still show their errors.
	await visit(page, '/create');
	await expect(page.getByText('HQ instance default')).toBeVisible();
	await create(page).click();
	await expect(page.getByText('Give the organization a name.')).toBeVisible();
	await expect(name).toBeFocused();
	await name.pressSequentially('Side Project');
	await expect(name).toBeFocused();
	await expect(name).toHaveValue('Side Project');
	await expect(slug).toHaveValue('side-project');
	await expect(repository).toHaveValue('');
	await expect(branch).toHaveValue('');
	await expect(page.getByText("Enter the remote repository's address.")).toBeVisible();
	await expect(page.getByText('Enter the state branch.')).toBeVisible();
	expect((await server.stats()).creates).toEqual([]);
});

test('a double click sends one request; Creating… is disabled; the created page shows the quoted recovery command and selects the organization (OFC-18)', async ({
	page
}) => {
	const server = await open(page);
	await fill(page, "Alex's Lab");
	await server.script('lookupScript', ['accepted', 'completed']);
	await create(page).dblclick();
	const creating = page.getByRole('button', { name: 'Creating…' });
	await expect(creating).toBeDisabled();
	await expect(page.getByLabel('Name', { exact: true })).not.toBeEditable();
	await expect(page.getByText("Closing this panel doesn't stop it.")).toBeVisible();
	await expect(page.getByRole('link', { name: 'Close' })).toHaveAttribute('href', '/welcome');
	await page.getByLabel('State branch').press('Enter');
	await expect(
		page.getByRole('heading', { name: "Created Alex's Lab. You own it." })
	).toBeFocused();
	await expect(
		page.getByText("Alex's Lab is starting. This usually takes a few seconds.")
	).toBeVisible();
	await expect(page.getByTestId('recovery-command')).toHaveText(
		"agentic-org recovery release --org 'Alex'\\''s Lab'"
	);
	const stats = await server.stats();
	expect(stats.creates).toHaveLength(1);
	expect(stats.creates[0].params.slug).toBe('alex-s-lab');
	expect(new Set(stats.lookups)).toEqual(new Set([stats.creates[0].operation_id]));
	expect(stats.lookups).toHaveLength(2);
	expect(await axe(page)).toEqual([]);
	const stored = await page.evaluate(() => localStorage.getItem('org-selection'));
	expect(Object.values(JSON.parse(stored!))).toEqual([
		`org-${stats.creates[0].operation_id.slice(-6).toLowerCase()}`
	]);
	expect(stored).not.toMatch(/proof|[0-9a-f]{64}/);
	await page.getByRole('button', { name: "Open Alex's Lab" }).click();
	await expect(page).not.toHaveURL(/\/create$/);
});

test('a name or slug HQ already has is named by its field, creates nothing, and the next attempt is new (OFC-17)', async ({
	page
}) => {
	const server = await open(page);
	await fill(page);
	await server.script('createScript', [
		{ rejected: { code: 'already_exists', field: 'name' }, status: 409 },
		{ rejected: { code: 'already_exists', field: 'slug' }, status: 409 },
		{ refused: { field: 'state_branch', rule: 'format' } },
		'completed'
	]);
	await create(page).click();
	const name = page.getByLabel('Name', { exact: true });
	await expect(
		page.getByText(
			"There's already an organization called Side Project on this HQ. Choose another name."
		)
	).toBeVisible();
	await expect(name).toHaveAttribute('aria-invalid', 'true');
	await expect(name).toHaveValue('Side Project');
	await expect(name).toBeEditable();
	await create(page).click();
	await expect(
		page.getByText(
			"There's already an organization with the slug side-project on this HQ. Choose another slug."
		)
	).toBeVisible();
	await expect(page.getByText(/already an organization called/)).toHaveCount(0);
	await create(page).click();
	await expect(page.getByText('Enter a branch name, for example main.')).toBeVisible();
	await page.getByLabel('State branch').fill('main');
	await expect(page.getByText('Enter a branch name, for example main.')).toHaveCount(0);
	await create(page).click();
	await expect(
		page.getByRole('heading', { name: 'Created Side Project. You own it.' })
	).toBeVisible();
	const ids = (await server.stats()).creates.map((c) => c.operation_id);
	expect(ids).toHaveLength(3);
	expect(new Set(ids).size).toBe(3);
});

test('a confirmed failure keeps the fields, focuses the alert, and offers Try again (a new attempt) and Copy details without secrets (OFC-20, FC-018 (7))', async ({
	page
}) => {
	await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
	const server = await open(page);
	await fill(page);
	await server.script('createScript', [
		{ rejected: { code: 'storage_error' }, status: 400 },
		'completed'
	]);
	await create(page).click();
	const alert = page.getByRole('alert').filter({ hasText: "wasn't created" });
	await expect(alert).toHaveText(
		/Side Project wasn't created: HQ couldn't write its data on the HQ machine\. Run `agentic-org doctor` there, then try again\./
	);
	await expect(alert).toBeFocused();
	await expect(page.getByLabel('Name', { exact: true })).toHaveValue('Side Project');
	await expect(page).toHaveURL(/\/create$/);
	expect(await axe(page)).toEqual([]);
	await alert.getByRole('button', { name: 'Copy details' }).click();
	await expect(alert.getByText('Copied.')).toBeVisible();
	const first = (await server.stats()).creates[0].operation_id;
	const details = await page.evaluate(() => navigator.clipboard.readText());
	expect(details.split('\n').map((line) => line.split(': ')[0])).toEqual([
		'Organization',
		'Attempt',
		'Error',
		'Time',
		'Version'
	]);
	expect(details).toContain(`Attempt: ${first}`);
	expect(details).toContain('Error: storage_error');
	expect(details).toContain('Version: 0.0.0-test');
	expect(details).not.toMatch(/[0-9a-f]{64}|proof|token|cookie|recovery/i);
	await alert.getByRole('button', { name: 'Try again' }).click();
	await expect(
		page.getByRole('heading', { name: 'Created Side Project. You own it.' })
	).toBeVisible();
	const ids = (await server.stats()).creates.map((c) => c.operation_id);
	expect(ids).toHaveLength(2);
	expect(ids[1]).not.toBe(first);
});

test('a lost reply shows Checking, looks the SAME attempt up and never creates twice (OFC-21)', async ({
	page
}) => {
	const server = await open(page);
	await fill(page);
	await server.script('createScript', ['lost']);
	// The first lookup gets no answer either, so Checking stays for one wait before HQ reports progress.
	await server.script('lookupScript', ['unavailable', 'accepted', 'completed']);
	await create(page).click();
	await expect(page.getByText('Checking whether Side Project was created…')).toBeVisible();
	await expect(page.getByText('Still creating Side Project…')).toBeVisible();
	await expect(
		page.getByRole('heading', { name: 'Created Side Project. You own it.' })
	).toBeVisible();
	const stats = await server.stats();
	expect(stats.creates).toHaveLength(1);
	expect(stats.lookups).toEqual(Array(3).fill(stats.creates[0].operation_id));
});

test('a 504 with an unreachable HQ offers only Check again; no record and a 503 re-send the SAME attempt (OFC-21)', async ({
	page
}) => {
	const server = await open(page);
	await fill(page);
	await server.script('createScript', ['unknown', 'unavailable', 'completed']);
	await server.script('lookupScript', ['unavailable', 'unavailable', 'unavailable', 'missing']);
	await create(page).click();
	await expect(page.getByText('Checking whether Side Project was created…')).toBeVisible();
	const alert = page.locator('form [role=alert]');
	await expect(alert).toHaveText(
		/Couldn't confirm whether Side Project was created, because HQ can't be reached\./,
		{ timeout: 20_000 }
	);
	await expect(alert.getByRole('button', { name: 'Try again' })).toHaveCount(0);
	await expect(create(page)).toBeDisabled();
	await alert.getByRole('button', { name: 'Check again' }).click();
	await expect(alert).toHaveText(/Side Project wasn't created\. Try again\./);
	expect((await server.stats()).creates).toHaveLength(1);
	await alert.getByRole('button', { name: 'Try again' }).click();
	await expect(alert).toHaveText(
		/HQ couldn't be asked to create Side Project\. Try again in a moment\./
	);
	await alert.getByRole('button', { name: 'Try again' }).click();
	await expect(
		page.getByRole('heading', { name: 'Created Side Project. You own it.' })
	).toBeVisible();
	const ids = (await server.stats()).creates.map((c) => c.operation_id);
	expect(ids).toHaveLength(3);
	expect(new Set(ids).size).toBe(1);
});

test("HQ's refusal for an account that isn't the administrator shows its copy with no Try again (OFC-22)", async ({
	page
}) => {
	const server = await open(page);
	await fill(page);
	await server.script('createScript', [{ rejected: { code: 'not_authorized' }, status: 403 }]);
	await create(page).click();
	const alert = page.locator('form [role=alert]');
	await expect(alert).toHaveText(/Only this HQ's administrator can create organizations\./);
	await expect(alert).toBeFocused();
	await expect(alert.getByRole('button', { name: 'Try again' })).toHaveCount(0);
	await expect(alert.getByRole('button', { name: 'Copy details' })).toBeVisible();
	// Trying again cannot change it: Create is disabled and Enter sends nothing.
	await expect(create(page)).toBeDisabled();
	await page.getByLabel('State branch').press('Enter');
	await expect(alert).toBeVisible();
	expect((await server.stats()).creates).toHaveLength(1);
});

test('an account that is not the administrator gets no form at /create (OFC-22)', async ({
	page
}) => {
	const context = page.context();
	const made = await context.request.post('/__p6-auth/new');
	const { id } = await made.json();
	await context.addCookies([
		{ name: 'p6_auth_case', value: id, url: new URL('/', made.url()).href }
	]);
	await visit(page, '/sign-in');
	await page.getByLabel('Username').fill('alex');
	await page.getByLabel('Password', { exact: true }).fill('private-password');
	await page.getByRole('button', { name: 'Sign in', exact: true }).click();
	await expect(page).toHaveURL(/\/welcome$/);
	await visit(page, '/create');
	await expect(
		page.getByText("Only this HQ's administrator can create organizations.")
	).toBeVisible();
	await expect(page.getByRole('textbox')).toHaveCount(0);
	await expect(create(page)).toHaveCount(0);
});

test('closing while creating leaves the page and sends nothing more: the create goes on at HQ (OFC-23)', async ({
	page
}) => {
	const server = await open(page);
	await fill(page);
	await server.script('lookupScript', ['accepted', 'accepted', 'accepted', 'accepted']);
	await create(page).click();
	await expect(page.getByRole('button', { name: 'Creating…' })).toBeDisabled();
	await page.getByRole('link', { name: 'Close' }).click();
	await expect(page).toHaveURL(/\/welcome$/);
	const stats = await server.stats();
	expect(stats.creates).toHaveLength(1);
	// No cancel route exists and none is called; the page's lookups stop with the page.
	const lookups = stats.lookups.length;
	await page.waitForTimeout(2500);
	expect((await server.stats()).lookups).toHaveLength(lookups);
});

test('signed out, /create opens sign-in and returns afterwards', async ({ page }) => {
	await visit(page, '/create');
	await expect(page).toHaveURL(/\/sign-in\?next=%2Fcreate$/);
});

// The signed-in organizations page on the real list (FC-018 (1), (4), (9)-(11)).
const LUMEN: WebOrganization = {
	id: 'org-lumen',
	name: 'Lumen Labs',
	created_at: '2026-10-02T12:00:00.000Z',
	recovery: 'acknowledged'
};
const SIDE: WebOrganization = {
	id: 'org-side',
	name: 'Side Project',
	created_at: '2026-11-10T12:00:00.000Z',
	recovery: 'pending'
};
const hqDown = (organization: WebOrganization) => {
	const view = organizationView(organization);
	return {
		...view,
		offices: [{ ...view.offices[0], status: 'unavailable', status_reason: 'crashed' }]
	};
};
const row = (page: Page, name: string) =>
	page.getByTestId('organization-row').filter({ hasText: name });

test('the organizations page lists each organization with its HQ office, host, created date and the open one; opening selects it (FC-018 (1))', async ({
	page
}) => {
	await signInAsAdmin(page, [LUMEN, SIDE]);
	await expect(page.getByRole('heading', { level: 1, name: 'Organizations' })).toBeVisible();
	await expect(page.getByTestId('organization-row')).toHaveCount(2);
	await expect(row(page, 'Lumen Labs')).toContainText('HQ · hq-mac');
	await expect(row(page, 'Lumen Labs')).toContainText(/Created\s+\S.*2026/);
	await expect(row(page, 'Side Project')).toContainText('HQ · hq-mac');
	await expect(page.getByText('Open now')).toHaveCount(0);
	await expect(page.getByRole('link', { name: 'Create organization' })).toBeVisible();
	expect(await axe(page)).toEqual([]);

	await page.getByRole('button', { name: 'Open Side Project' }).click();
	await expect(row(page, 'Side Project')).toContainText('Open now');
	await expect(row(page, 'Lumen Labs')).not.toContainText('Open now');
	// Only IDs are kept, and the choice survives a reload.
	const stored = await page.evaluate(() => localStorage.getItem('org-selection'));
	expect(Object.values(JSON.parse(stored!))).toEqual(['org-side']);
	expect(stored).not.toContain('Side Project');
	await visit(page, '/welcome');
	await expect(row(page, 'Side Project')).toContainText('Open now');
});

test('an organization created here is listed afterwards and marked open now (FC-018 (4))', async ({
	page
}) => {
	await signInAsAdmin(page, [LUMEN]);
	await page.getByRole('link', { name: 'Create organization' }).click();
	await expect(page.getByText('HQ instance default')).toBeVisible();
	await fill(page);
	await create(page).click();
	await expect(
		page.getByRole('heading', { name: 'Created Side Project. You own it.' })
	).toBeVisible();
	await visit(page, '/welcome');
	await expect(page.getByTestId('organization-row')).toHaveCount(2);
	await expect(row(page, 'Side Project')).toContainText('Open now');
	await expect(row(page, 'Lumen Labs')).not.toContainText('Open now');
});

test('a create still in progress is shown as being created and resumes its lookup until it is listed', async ({
	page
}) => {
	await signInAsAdmin(page, [LUMEN]);
	const server = await backend(page);
	const operation_id = '01M72N8MG00000000000000006';
	await server.set('creating', [{ operation_id, name: 'Side Project', slug: 'side-project' }]);
	await server.script('lookupScript', ['accepted']);
	await visit(page, '/welcome');
	const pending = page.getByTestId('organization-creating');
	await expect(pending).toContainText('Side Project');
	await expect(pending).toContainText('Being created…');
	await expect(pending.getByRole('button')).toHaveCount(0);
	// The second lookup completes: the entry becomes a listed organization.
	await expect(row(page, 'Side Project')).toBeVisible();
	await expect(pending).toHaveCount(0);
	const stats = await server.stats();
	expect(stats.lookups).toEqual([operation_id, operation_id]);
	expect(stats.creates.filter((c) => c.operation_id !== operation_id)).toEqual([]);
});

test("a create whose lookups get no answer keeps its row, says Can't reach HQ, and Check again follows it to the end", async ({
	page
}) => {
	await signInAsAdmin(page, [LUMEN]);
	const server = await backend(page);
	const operation_id = '01M72N8MG00000000000000006';
	await server.set('creating', [{ operation_id, name: 'Side Project', slug: 'side-project' }]);
	await server.script('lookupScript', ['unavailable', 'unavailable', 'unavailable', 'completed']);
	await visit(page, '/welcome');
	const pending = page.getByTestId('organization-creating');
	await expect(pending).toContainText('Being created…');
	await expect(pending).toContainText("Can't reach HQ", { timeout: 20_000 });
	await expect(pending).toContainText('Side Project');
	await expect(pending).not.toContainText('Being created…');
	expect((await server.stats()).lookups).toHaveLength(3);
	expect(await axe(page)).toEqual([]);
	// Nothing is asked until the person asks.
	await page.waitForTimeout(1500);
	expect((await server.stats()).lookups).toHaveLength(3);
	await pending.getByRole('button', { name: 'Check again on Side Project' }).click();
	await expect(row(page, 'Side Project')).toBeVisible();
	await expect(pending).toHaveCount(0);
	const stats = await server.stats();
	expect(stats.lookups).toEqual(Array(4).fill(operation_id));
	expect(stats.creates.filter((c) => c.operation_id !== operation_id)).toEqual([]);
});

test("when HQ can't be reached the page explains why and still offers Create; Try again reads the list (FC-018 (9))", async ({
	page
}) => {
	await signInAsAdmin(page, [LUMEN]);
	const server = await backend(page);
	await server.set('listUnavailable', true);
	await visit(page, '/welcome');
	await expect(page.getByRole('alert')).toContainText(
		"Can't reach HQ. Check that the instance is running on the HQ machine, then try again."
	);
	await expect(page.getByTestId('organization-row')).toHaveCount(0);
	await expect(page.getByRole('link', { name: 'Create organization' })).toBeVisible();
	expect(await axe(page)).toEqual([]);
	await server.set('listUnavailable', false);
	await page.getByRole('button', { name: 'Try again' }).click();
	await expect(row(page, 'Lumen Labs')).toBeVisible();
});

test('an organization that vanished after the list was shown is dropped when opening it fails, staying on the list (FC-018 (10))', async ({
	page
}) => {
	await signInAsAdmin(page, [LUMEN, SIDE]);
	const server = await backend(page);
	await server.set('organizations', [organizationView(LUMEN)]);
	await page.getByRole('button', { name: 'Open Side Project' }).click();
	const alert = page.getByRole('alert');
	await expect(alert).toContainText("Couldn't open it");
	await expect(alert).toContainText('Side Project is no longer on this HQ.');
	await expect(alert).toBeFocused();
	await expect(page).toHaveURL(/\/welcome$/);
	await expect(page.getByTestId('organization-row')).toHaveCount(1);
	await expect(row(page, 'Lumen Labs')).toBeVisible();
	expect(await page.evaluate(() => localStorage.getItem('org-selection'))).toBeNull();
});

test('an organization whose HQ is unavailable is listed, marked Unavailable and cannot be opened, also when it goes down after the list was shown (FC-018 (11))', async ({
	page
}) => {
	await signInAsAdmin(page, [LUMEN, SIDE]);
	const server = await backend(page);
	await server.set('organizations', [hqDown(LUMEN), organizationView(SIDE)]);
	await visit(page, '/welcome');
	await expect(row(page, 'Lumen Labs')).toContainText('Unavailable');
	await expect(page.getByRole('button', { name: 'Open Lumen Labs' })).toBeDisabled();
	await expect(row(page, 'Side Project')).not.toContainText('Unavailable');
	// It goes down after the list was shown: opening reads the list again and refuses.
	await server.set('organizations', [hqDown(LUMEN), hqDown(SIDE)]);
	await page.getByRole('button', { name: 'Open Side Project' }).click();
	await expect(page.getByRole('alert')).toContainText(
		"Side Project's HQ office is unavailable right now."
	);
	await expect(row(page, 'Side Project')).toContainText('Unavailable');
	await expect(page.getByRole('button', { name: 'Open Side Project' })).toBeDisabled();
	expect(await page.evaluate(() => localStorage.getItem('org-selection'))).toBeNull();
});
