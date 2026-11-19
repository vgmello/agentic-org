// Organization settings › Processes (processes-ux §2; PROC-*) and development processes (development-processes
// design §2), against the dev mock. `?mock=` on the first address selects a state: slow, fails, empty, unavailable,
// stale, viewer, conflict, save-fails (e2e/fakes/modules/processes.ts).
import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { enterMockedOrganization } from './mock-organization';
import { shot } from './shot';

async function open(page: Page, url = '/settings/processes') {
	await enterMockedOrganization(page, 'Acme');
	await visit(page, url);
}
const row = (page: Page, key: string) =>
	page.locator(`[data-testid=process-row][data-key="${key}"]`);
const heading = (page: Page) => page.locator('#processes-heading');
const show = (page: Page) => page.getByLabel('Show processes for');

test(
	'PROC-9: the organization’s processes and standards, as two tabs with their definitions',
	{ tag: '@journey' },
	async ({ page }) => {
		await open(page);
		await expect(heading(page)).toHaveText('Processes · Organization');
		await expect(page).toHaveTitle('Organization · Processes · Agentic Org');
		await expect(page.getByTestId('definition')).toHaveText(
			'Processes: How the organization handles something, whatever is being built'
		);
		await expect(page.getByTestId('process-row')).toHaveCount(4);
		const security = row(page, 'security-basics');
		await expect(security.getByTestId('level-badge')).toHaveText('Organization rev 1');
		await expect(security.getByTestId('locked')).toHaveText('not overridable');
		await expect(security.getByTestId('applies')).toHaveText('Applies');
		await expect(row(page, 'review-and-merge').getByTestId('level-badge')).toHaveText(
			'Organization rev 3'
		);
		// Drafts and deprecated documents are listed apart and never applied.
		await expect(page.getByTestId('inactive-row')).toHaveCount(2);
		await expect(page.getByTestId('inactive-row').first()).toContainText('Draft');
		// A development process is never a row here (PROC-37); the page names it apart.
		await expect(page.getByRole('link', { name: 'development process' })).toHaveAttribute(
			'href',
			'/settings/processes/development'
		);
		await shot(page, 'processes-organization');

		await page.getByRole('link', { name: /^Standards/ }).click();
		await expect(page).toHaveURL(/kind=standard/);
		await expect(page.getByTestId('definition')).toHaveText(
			'Standards: What the organization uses, and how it builds and runs things'
		);
		await expect(page.getByTestId('process-row')).toHaveCount(3);
		await expect(row(page, 'deployment-pattern').getByTestId('locked')).toBeVisible();
		await expect(page.getByRole('link', { name: /^Standards/ })).toHaveAttribute(
			'aria-current',
			'page'
		);
	}
);

test(
	'PROC-8: a process opens with its text and why: every level and what happened to it',
	{ tag: '@journey' },
	async ({ page }) => {
		await open(page, '/settings/processes?place=team:web');
		const review = row(page, 'code-review');
		await expect(review.getByTestId('level-badge')).toHaveText('Team Web rev 2');
		await expect(review.getByTestId('relation')).toHaveText('replaces Organization');
		const opener = review.getByRole('button', { name: 'Code review' });
		await opener.click();
		const sheet = page.getByRole('dialog');
		await expect(sheet.getByRole('heading', { name: 'Code review' })).toBeVisible();
		const layers = sheet.getByTestId('why-layer');
		await expect(layers).toHaveCount(2);
		await expect(layers.nth(0)).toContainText('Organization rev 1');
		await expect(layers.nth(0)).toContainText('Replaced by Team Web.');
		await expect(layers.nth(1)).toContainText('Applies');
		await expect(sheet.getByText('Attach a screenshot of any UI change.')).toBeVisible();
		await sheet.getByRole('button', { name: 'Compare with Organization' }).click();
		// The new step is added; the next one is renumbered, so it shows as removed and added.
		const diff = sheet.getByTestId('diff');
		await expect(diff.locator('[data-kind=added]').first()).toContainText(
			'Added: 3. Attach a screenshot of any UI change.'
		);
		await expect(diff.locator('[data-kind=removed]')).toHaveCount(1);
		await expect(diff.locator('[data-kind=removed]')).toContainText(
			'Removed: 3. Say what must change and what is only a suggestion.'
		);
		await expect(diff.getByText('2 lines added, 1 line removed')).toBeVisible();
		await shot(page, 'processes-why', { context: sheet });
		await sheet.getByRole('button', { name: 'Close', exact: true }).click();
		await expect(page.getByRole('dialog')).toHaveCount(0);
		await expect(opener).toBeFocused();
	}
);

test(
	'PROC-31/32/34: Show is a history entry; the URL carries the place; Back, Forward and reload keep it',
	{ tag: '@webkit' },
	async ({ page }) => {
		await open(page);
		await expect(page.getByTestId('process-row').first()).toBeVisible();
		const labels = await show(page).locator('option').allTextContents();
		expect(labels[0]).toBe('Organization');
		expect(labels).toEqual(
			expect.arrayContaining(['Web (team)', 'Atlas (project)', 'Noor Haddad (agent)'])
		);
		// Teams come before projects, and projects before agents.
		expect(labels.indexOf('Web (team)')).toBeLessThan(labels.indexOf('Atlas (project)'));
		expect(labels.indexOf('Atlas (project)')).toBeLessThan(labels.indexOf('Noor Haddad (agent)'));

		await show(page).focus();
		await show(page).selectOption('team:web');
		await expect(page).toHaveURL(/place=team%3Aweb/);
		await expect(heading(page)).toHaveText('Processes · Web (team)');
		await expect(page).toHaveTitle('Web (team) · Processes · Agentic Org');
		await expect(show(page)).toBeFocused();
		await show(page).selectOption('project:atlas');
		await expect(heading(page)).toHaveText('Processes · Atlas (project)');
		await page.goBack();
		await expect(heading(page)).toHaveText('Processes · Web (team)');
		await expect(show(page)).toHaveValue('team:web');
		await page.goForward();
		await expect(heading(page)).toHaveText('Processes · Atlas (project)');
		await page.reload();
		await expect(heading(page)).toHaveText('Processes · Atlas (project)');
		// A project's development process is named above the list and links to it (PROC-35).
		await expect(page.getByTestId('dev-process-line')).toHaveText(
			'This project follows Desired State Specification (Payments). Its skills are shown there.'
		);
		// Atlas's own standard replaces the organization's; the tab keeps the place.
		await page.getByRole('link', { name: /^Standards/ }).click();
		await expect(page).toHaveURL(/place=project%3Aatlas&kind=standard/);
		await expect(row(page, 'observability').getByTestId('relation')).toHaveText(
			'replaces Organization'
		);
		await expect(row(page, 'technology-stack').getByTestId('level-badge')).toHaveText(
			'Team Platform rev 1'
		);
	}
);

test('PROC-3/PROC-16: a refused replacement, details not recognized with Fix, and a key clash with Rename…', async ({
	page
}) => {
	await open(page, '/settings/processes?place=team:platform');
	await expect(
		page.getByTestId('problem').filter({
			hasText:
				"Platform's version of Security basics isn't applied: the organization marked it not overridable. You can add a process instead."
		})
	).toBeVisible();
	await expect(row(page, 'security-basics').first().getByTestId('applies')).toHaveText('Applies');

	await show(page).selectOption('team:web');
	const handoff = row(page, 'design-handoff');
	await expect(handoff.getByTestId('problem')).toHaveText(
		'Design handoff: details not recognized (overridable).'
	);
	await expect(handoff.getByTestId('applies')).toHaveText('Not applied');
	await expect(handoff.getByRole('link', { name: 'Fix Design handoff' })).toBeVisible();
	await shot(page, 'processes-problems');

	await show(page).selectOption('project:beacon');
	const clash = page.getByTestId('problem').filter({
		hasText: 'Two processes use the key standup in Beacon. Neither applies until one is renamed.'
	});
	await expect(clash).toHaveCount(2);
	await page.getByRole('button', { name: 'Rename Daily standup…' }).click();
	const dialog = page.getByRole('dialog');
	await expect(dialog.getByLabel('Key')).toBeFocused();
	await dialog.getByLabel('Key').fill('standup');
	await dialog.getByRole('button', { name: 'Rename' }).click();
	await expect(dialog.getByRole('alert')).toContainText('That is the key it has now');
	await dialog.getByLabel('Key').fill('daily-standup');
	await dialog.getByRole('button', { name: 'Rename' }).click();
	await expect(page.getByRole('dialog')).toHaveCount(0);
	await expect(clash).toHaveCount(0);
	await expect(row(page, 'standup').getByTestId('applies')).toHaveText('Applies');
	await expect(row(page, 'daily-standup').getByTestId('applies')).toHaveText('Applies');
});

test('PROC-1/PROC-3: override an inherited process for a team, with the inherited text as the start', async ({
	page
}) => {
	await open(page, '/settings/processes?place=team:web');
	await row(page, 'review-and-merge').getByRole('button', { name: 'Review and merge' }).click();
	await page.getByRole('dialog').getByRole('link', { name: 'Override for Web' }).click();
	await expect(page.locator('#editor-heading')).toHaveText('Override Review and merge');
	await expect(page).toHaveTitle('Override Review and merge · Processes · Agentic Org');
	await expect(page.getByLabel('Key (optional)')).toHaveValue('review-and-merge');
	await expect(page.getByLabel('Key (optional)')).toHaveAttribute('readonly', '');
	const text = page.getByRole('textbox', { name: 'Text' });
	await expect(text).toHaveValue(/Merged when its checks pass/);
	await text.fill(`${await text.inputValue()}\nWeb also needs a screenshot.\n`);
	await page.getByRole('button', { name: 'Compare with Organization' }).click();
	await expect(
		page
			.getByTestId('diff')
			.locator('[data-kind=added]', { hasText: 'Web also needs a screenshot.' })
	).toHaveCount(1);
	await shot(page, 'processes-override-editor');
	await page.getByRole('button', { name: 'Create process' }).click();
	await expect(page).toHaveURL(/\/settings\/processes\?place=team%3Aweb$/);
	await expect(page.getByTestId('arrival')).toHaveText('Created Review and merge for Web (team).');
	await expect(
		row(page, 'review-and-merge').getByRole('button', { name: 'Review and merge' })
	).toBeFocused();
	const review = row(page, 'review-and-merge');
	await expect(review.getByTestId('level-badge')).toHaveText('Team Web rev 1');
	await expect(review.getByTestId('relation')).toHaveText('replaces Organization');
});

test('PROC-3: a save that would replace a document marked not overridable is refused on the key, with the rule', async ({
	page
}) => {
	await open(page, '/settings/processes/documents/new?place=team:platform&kind=process');
	await expect(page.locator('#editor-heading')).toHaveText('New process');
	await page.getByLabel('Title').fill('Our PII handling');
	await page.getByLabel('Key (optional)').fill('pii-handling');
	await page.getByLabel('Owner').selectOption('kai');
	await page.getByRole('button', { name: 'Create process' }).click();
	await expect(page.getByTestId('key-error')).toHaveText(
		"PII handling is marked not overridable by the organization, so it can't be replaced here. Use another key to add a process instead."
	);
	await expect(page.getByLabel('Key (optional)')).toBeFocused();
	await expect(page.getByLabel('Title')).toHaveValue('Our PII handling');
	// A current document needs an owner before anything is sent.
	await page.getByLabel('Key (optional)').fill('pii-notes');
	await page.getByLabel('Owner').selectOption('');
	await page.getByRole('button', { name: 'Create process' }).click();
	await expect(page.getByText('A current document needs an owner.')).toBeVisible();
	await expect(page.getByLabel('Owner')).toBeFocused();
});

test('PROC-16: Fix opens the document at its problem and saving brings it back into the set', async ({
	page
}) => {
	await open(page, '/settings/processes?place=team:web');
	await page.getByRole('link', { name: 'Fix Design handoff' }).click();
	await expect(page.locator('#editor-heading')).toHaveText('Fix Design handoff');
	await expect(page.getByTestId('fixing')).toContainText(
		'Design handoff: details not recognized (overridable).'
	);
	await page.getByRole('button', { name: 'Save process' }).click();
	await expect(heading(page)).toHaveText('Processes · Web (team)');
	await expect(row(page, 'design-handoff').getByTestId('applies')).toHaveText('Applies');
	await expect(row(page, 'design-handoff').getByTestId('level-badge')).toHaveText('Team Web rev 2');
});

test('PROC-10: an agent’s place shows its current work, or that it has none', async ({ page }) => {
	await open(page, '/settings/processes?place=agent:noor');
	await expect(heading(page)).toHaveText('Processes · Noor Haddad (agent)');
	await expect(page.getByTestId('agent-work')).toHaveText(
		"No current work. Showing the organization's processes. Team and project processes apply when it starts a task."
	);
	await expect(page.getByTestId('process-row')).toHaveCount(4);
	// An agent's place writes nothing: no New process.
	await expect(page.getByRole('link', { name: 'New process' })).toHaveCount(0);
	await show(page).selectOption('agent:kai');
	await expect(page.getByTestId('agent-work')).toHaveText(
		'What Kai Lindqvist follows for T-A37 Make the deploy script give up after 5 tries, and where each comes from.'
	);
	await expect(page.getByTestId('dev-process-line')).toContainText(
		'Desired State Specification (Payments)'
	);
	await expect(row(page, 'release-checklist').getByTestId('level-badge')).toHaveText(
		'Team Platform rev 1'
	);
});

test('PROC-16: nothing written yet invites the review-and-merge template', async ({ page }) => {
	await open(page, '/settings/processes?mock=empty');
	await expect(page.getByRole('heading', { name: 'No processes yet' })).toBeVisible();
	await expect(
		page.getByText(
			'Write down how your agents work. Start with how changes get reviewed and merged.'
		)
	).toBeVisible();
	await shot(page, 'processes-empty');
	await page.getByRole('link', { name: 'Use the review-and-merge template' }).click();
	await expect(page.getByLabel('Title')).toHaveValue('Review and merge');
	await expect(page.getByLabel('Key (optional)')).toHaveValue('review-and-merge');
	await expect(page.getByRole('textbox', { name: 'Text' })).toHaveValue(
		/Ask a teammate who did not write the change/
	);
});

test('PROC-16: loading, a failed read with Retry, unavailable, the last copy, a place that isn’t here, a viewer', async ({
	page
}) => {
	await open(page, '/settings/processes?mock=slow');
	await expect(page.getByRole('status').filter({ hasText: 'Loading processes' })).toBeVisible();
	await shot(page, 'processes-loading', { settle: false });

	await visit(page, '/settings/processes?mock=fails');
	await expect(page.getByRole('heading', { name: "Couldn't load the processes" })).toBeVisible();
	await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();

	await visit(page, '/settings/processes?mock=unavailable');
	await expect(page.getByText("Processes isn't built yet")).toBeVisible();
	await expect(page.getByTestId('process-row')).toHaveCount(0);

	await visit(page, '/settings/processes?mock=stale');
	await expect(page.getByTestId('stale')).toContainText(
		"Can't reach the organization repository. Showing the last copy from"
	);
	await expect(page.getByTestId('process-row')).toHaveCount(4);
	await expect(page.getByRole('button', { name: 'New process' })).toBeDisabled();
	await expect(page.getByTestId('paused-reason')).toHaveText(
		'Changes wait until the organization repository can be reached.'
	);
	await shot(page, 'processes-stale');

	await visit(page, '/settings/processes?place=team:zzz');
	await expect(page.getByRole('heading', { name: "That place isn't here" })).toBeVisible();

	await visit(page, '/settings/processes?mock=viewer&place=team:web');
	await expect(page.getByTestId('process-row').first()).toBeVisible();
	await expect(page.getByRole('link', { name: 'New process' })).toHaveCount(0);
	await expect(page.getByRole('link', { name: 'Fix Design handoff' })).toHaveCount(0);
});

test('an edit that loses a race keeps the text and says what saving again does', async ({
	page
}) => {
	await open(page, '/settings/processes');
	await row(page, 'code-review').getByRole('button', { name: 'Code review' }).click();
	const edit = page.getByRole('dialog').getByRole('link', { name: 'Edit Code review' });
	// Someone else saves first (the mock's `conflict`, once).
	await visit(page, `${await edit.getAttribute('href')}?mock=conflict`);
	await page.getByLabel('Title').fill('Code review, carefully');
	await page.getByRole('button', { name: 'Save process' }).click();
	await expect(page.getByTestId('save-failure')).toContainText(
		'saved rev 1 while you were editing. Your text is kept; saving again replaces theirs.'
	);
	await expect(page.getByLabel('Title')).toHaveValue('Code review, carefully');
	await page.getByRole('button', { name: 'Save over rev 1' }).click();
	await expect(row(page, 'code-review').getByRole('button')).toHaveText('Code review, carefully');
});

test('PKG-3/PKG-16: the catalogue of development processes; a built-in one is copied, the copy is edited', async ({
	page
}) => {
	await open(page, '/settings/processes/development');
	await expect(page).toHaveTitle('Development processes · Processes · Agentic Org');
	await expect(page.locator('#dev-heading')).toContainText(
		'Development processes: how a project builds software'
	);
	const rows = page.getByTestId('dev-row');
	await expect(rows).toHaveCount(4);
	await expect(rows.nth(0)).toContainText('Built in');
	await expect(page.locator('[data-testid=dev-row][data-key=research-spike]')).toContainText(
		'Draft'
	);
	await expect(page.locator('[data-testid=dev-row][data-key=dss-payments]')).toContainText(
		'Followed by'
	);
	await shot(page, 'dev-catalogue');

	await page.getByRole('link', { name: 'Desired State Specification', exact: true }).click();
	await expect(page.getByTestId('built-in')).toContainText("It can't be edited here");
	await expect(page.getByRole('button', { name: 'Edit process.yaml' })).toHaveCount(0);
	await expect(page.getByTestId('dev-skill')).toHaveCount(4);
	await page.getByRole('button', { name: 'Read the skill Write the Product DSS' }).click();
	await expect(
		page.getByRole('dialog').getByText('Talk with the person until the behaviour is clear.')
	).toBeVisible();
	await page.keyboard.press('Escape');
	await shot(page, 'dev-built-in');

	const download = page.waitForEvent('download');
	await page.getByRole('button', { name: 'Download' }).click();
	expect((await download).suggestedFilename()).toBe('dss.process.zip');

	await page.getByRole('button', { name: 'Copy…' }).click();
	const copy = page.getByRole('dialog');
	await copy.getByLabel('Key').fill('dss');
	await copy.getByRole('button', { name: 'Copy' }).click();
	await expect(copy.getByText('The key dss is taken. Choose another.')).toBeVisible();
	await copy.getByLabel('Key').fill('dss-web');
	await copy.getByLabel('Name').fill('DSS for Web');
	await copy.getByRole('button', { name: 'Copy' }).click();
	await expect(page.locator('#dev-process-heading')).toHaveText('DSS for Web');
	await expect(page.getByTestId('lifecycle')).toContainText('Draft');

	// PKG-2: a field the format doesn't have is refused, naming it; the text stays.
	await page.getByRole('button', { name: 'Edit process.yaml' }).click();
	const sheet = page.getByRole('dialog');
	const yaml = sheet.getByLabel('process.yaml');
	await yaml.fill(`${await yaml.inputValue()}budget: 500\n`);
	await sheet.getByRole('button', { name: 'Save' }).click();
	await expect(sheet.getByTestId('part-error')).toHaveText(
		"process.yaml: budget isn't a field a development process may have."
	);
	await yaml.fill(
		(await yaml.inputValue())
			.replace('budget: 500\n', '')
			.replace('status: draft', 'status: current')
	);
	await sheet.getByRole('button', { name: 'Save' }).click();
	await expect(page.getByTestId('saved')).toContainText('rev 2');
	await expect(page.getByTestId('lifecycle')).toHaveText('Current');

	// A skill names only roles of its own process.
	await page.getByRole('button', { name: 'Edit the skill Staff the project' }).click();
	const skill = page.getByRole('dialog').getByRole('textbox', { name: 'Text' });
	await skill.fill(
		(await skill.inputValue()).replace('roles: [product-lead, technical-lead]', 'roles: [designer]')
	);
	await page.getByRole('dialog').getByRole('button', { name: 'Save' }).click();
	await expect(page.getByTestId('part-error')).toHaveText(
		"skills/staff-the-project.md: designer isn't a role of this development process."
	);
});

test('PKG-16: a new development process starts as a draft; an imported file arrives as a draft', async ({
	page
}) => {
	await open(page, '/settings/processes/development');
	await page.getByRole('button', { name: 'New development process…' }).click();
	const dialog = page.getByRole('dialog');
	await dialog.getByLabel('Name').fill('Lemonade stand');
	await dialog.getByLabel('Key').fill('Lemonade');
	await dialog.getByRole('button', { name: 'Create' }).click();
	await expect(dialog.getByLabel('Key')).toBeFocused();
	await dialog.getByLabel('Key').fill('lemonade-stand');
	await dialog.getByRole('button', { name: 'Create' }).click();
	await expect(page.locator('#dev-process-heading')).toHaveText('Lemonade stand');
	await expect(page.getByTestId('lifecycle')).toContainText('Draft');

	await visit(page, '/settings/processes/development');
	await page.getByRole('button', { name: 'Import…' }).click();
	const importer = page.getByRole('dialog');
	await importer.getByLabel('Process file').setInputFiles({
		name: 'notes.txt',
		mimeType: 'text/plain',
		buffer: Buffer.from('not a process')
	});
	await importer.getByRole('button', { name: 'Import' }).click();
	await expect(importer.getByText("notes.txt isn't a development process file.")).toBeVisible();
	await importer.getByLabel('Process file').setInputFiles({
		name: 'spike.process.zip',
		mimeType: 'application/zip',
		buffer: Buffer.from(
			JSON.stringify({
				key: 'research-spike',
				name: 'Research spike',
				roles: [],
				skills: [],
				templates: [],
				stages: [],
				kinds: [],
				folders: [],
				tabs: [],
				summary: ''
			})
		)
	});
	await importer.getByRole('button', { name: 'Import' }).click();
	await expect(
		importer.getByText('The key research-spike is taken. Choose another key for it.')
	).toBeVisible();
	await importer.getByLabel('Key (optional)').fill('research-spike-2');
	await importer.getByRole('button', { name: 'Import' }).click();
	await expect(page.locator('#dev-process-heading')).toHaveText('Research spike');
	await expect(page.getByTestId('lifecycle')).toContainText('Draft');
});

test('a team’s and an agent’s Processes tabs read through the processes module', async ({
	page
}) => {
	await open(page, '/teams/web/processes');
	await expect(page.getByRole('heading', { name: /^Organization processes/ })).toBeVisible();
	await expect(row(page, 'code-review').getByTestId('relation')).toHaveText(
		'replaces Organization'
	);
	await expect(page.getByRole('link', { name: 'Open in Processes' })).toHaveAttribute(
		'href',
		'/settings/processes?place=team%3Aweb'
	);
	await visit(page, '/members/noor/processes');
	await expect(page.getByTestId('agent-work')).toHaveText(
		"No current work. Showing the organization's processes. Team and project processes apply when it starts a task."
	);
});

test('review P2-4: a document written for another place lands on that place’s list, its row focused', async ({
	page
}) => {
	await open(page, '/settings/processes');
	await page.getByRole('link', { name: 'New process' }).click();
	await page.getByLabel('Title').fill('Web review');
	await page.getByLabel('Written for').selectOption('team:web');
	await page.getByLabel('Owner').selectOption('noor');
	await page.getByRole('button', { name: 'Create process' }).click();
	await expect(page).toHaveURL(/\/settings\/processes\?place=team%3Aweb$/);
	await expect(heading(page)).toHaveText('Processes · Web (team)');
	await expect(page.getByTestId('arrival')).toHaveText('Created Web review for Web (team).');
	await expect(page.getByRole('button', { name: 'Web review', exact: true })).toBeFocused();
});

test('review P2-3: every refusal is shown: on the lifecycle, and a form-level one for a field the editor has no place for', async ({
	page
}) => {
	await open(page, '/settings/processes');
	await visit(page, '/settings/processes/documents/new?kind=process&mock=refuse-status');
	await page.getByLabel('Title').fill('Asking for help');
	await page.getByLabel('Owner').selectOption('avery');
	await page.getByRole('button', { name: 'Create process' }).click();
	await expect(
		page.getByText(
			'The owner has left the organization. Save it as a draft, or choose another owner.'
		)
	).toBeVisible();
	await expect(page.locator('fieldset[role=radiogroup]')).toHaveAttribute('aria-invalid', 'true');
	await expect(page.getByRole('radio', { name: /^Draft/ })).toBeFocused();

	await visit(
		page,
		'/settings/processes/documents/new?place=team:web&kind=process&mock=refuse-scope'
	);
	await page.getByLabel('Title').fill('Web notes');
	await page.getByLabel('Owner').selectOption('noor');
	await page.getByRole('button', { name: 'Create process' }).click();
	await expect(page.getByTestId('save-failure')).toHaveText(
		'Web is archived, so nothing can be written there.'
	);
	await expect(page.getByLabel('Title')).toHaveValue('Web notes');
});

test('review P2-2: a new role, skill or template with a taken key is refused on the key, never written over', async ({
	page
}) => {
	await open(page, '/settings/processes/development/0193a000-0000-7000-8000-0000000000d0');
	await expect(page.getByTestId('dev-role')).toHaveCount(4);
	await page.getByRole('button', { name: 'New role…' }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByLabel('Key').fill('product-lead');
	await sheet.getByRole('button', { name: 'Save' }).click();
	await expect(
		sheet.getByText('roles/product-lead.md already exists. Edit it, or choose another key.')
	).toBeVisible();
	await expect(sheet.getByLabel('Key')).toBeFocused();
	await sheet.getByLabel('Key').fill('qa-lead');
	await sheet.getByRole('button', { name: 'Save' }).click();
	await expect(page.getByTestId('saved')).toContainText('roles/qa-lead.md');
	await expect(page.getByTestId('dev-role')).toHaveCount(5);

	// A new skill's front matter is saved under the key typed for it.
	await page.getByRole('button', { name: 'New skill…' }).click();
	await page.getByRole('dialog').getByLabel('Key').fill('pci-review');
	await page.getByRole('dialog').getByRole('button', { name: 'Save' }).click();
	await expect(page.getByText('skills/pci-review.md already exists.')).toBeVisible();
	await page.getByRole('dialog').getByLabel('Key').fill('qa-checks');
	await page.getByRole('dialog').getByRole('button', { name: 'Save' }).click();
	await page.getByRole('button', { name: 'Edit the skill New skill' }).click();
	await expect(page.getByRole('dialog').getByRole('textbox', { name: 'Text' })).toHaveValue(
		/^---\nschema_version: 1\nkey: qa-checks\n/
	);
});

test('review P3: no frame around an unavailable page; no zero counts; Back and Forward keep focus off the body', async ({
	page
}) => {
	await open(page, '/settings/processes?mock=unavailable');
	await expect(page.getByText("Processes isn't built yet")).toBeVisible();
	await expect(page.getByRole('navigation', { name: 'Processes and standards' })).toHaveCount(0);
	await expect(page.getByLabel('Show processes for')).toHaveCount(0);
	await expect(page.locator('title')).toHaveCount(1);
	await expect(page).toHaveTitle('Processes · Organization settings · Agentic Org');

	await visit(page, '/settings/processes?mock=empty');
	await expect(
		page
			.getByRole('navigation', { name: 'Processes and standards' })
			.getByRole('link', { name: 'Processes', exact: true })
	).toHaveText('Processes');

	await visit(page, '/settings/processes');
	await expect(page.getByTestId('process-row').first()).toBeVisible();
	await show(page).selectOption('team:web');
	await expect(heading(page)).toHaveText('Processes · Web (team)');
	await page.locator('body').click({ position: { x: 1, y: 1 } });
	await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
	await page.goBack();
	await expect(heading(page)).toHaveText('Processes · Organization');
	await expect(heading(page)).toBeFocused();
});
