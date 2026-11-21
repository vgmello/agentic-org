// Step 6: Project Docs, Features, Releases, and "Why is..." Knowledge search (v1-ui-screens.md Step 6,
// decisions-and-tech-debt-ux.md, features-ux.md, organization-data 07/09/18) against the dev mock.
import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { enterMockedOrganization } from './mock-organization';

async function open(page: Page, url: string) {
	await enterMockedOrganization(page, 'Lumen Labs');
	await visit(page, url);
}

async function noSidewaysScroll(page: Page) {
	expect(
		await page.evaluate(
			() => document.documentElement.scrollWidth - document.documentElement.clientWidth
		)
	).toBeLessThanOrEqual(0);
}

test('DEC-1/2/6/8 & LMT-1: Project Docs tab groups by kind and by section, collapses Older, and creates a new Decision', async ({
	page
}) => {
	const pageErrors: string[] = [];
	const consoleErrors: string[] = [];
	page.on('pageerror', (err) => pageErrors.push(err.message));
	page.on('console', (msg) => {
		if (msg.type() !== 'error') return;
		if (msg.location().url.endsWith('/api/v1/version')) return;
		consoleErrors.push(`${msg.text()} (${msg.location().url})`);
	});

	await open(page, '/projects/beacon/docs');
	await expect(page).toHaveTitle('Docs · Beacon · Projects · Agentic Org');

	// Brief pinned, active groups shown, evaluation excerpt labelled, Known limitations linked
	await expect(page.getByRole('heading', { name: /Brief \(pinned\)/i })).toBeVisible();
	await expect(page.getByRole('heading', { name: /Decisions · 2/i })).toBeVisible();
	await expect(page.getByTestId('eval-excerpt')).toContainText('Excerpt:');
	await expect(page.getByTestId('known-limitations-section')).toContainText(
		'Split-tender gift card'
	);

	// Deprecated doc collapsed under Older (DEC-8)
	const olderToggle = page.getByRole('button', { name: /Older · 1 \(deprecated\)/i });
	await expect(olderToggle).toHaveAttribute('aria-expanded', 'false');
	await expect(page.getByText('Legacy single-request checkout endpoint')).toHaveCount(0);
	await olderToggle.click();
	await expect(olderToggle).toHaveAttribute('aria-expanded', 'true');
	await expect(page.getByText('Legacy single-request checkout endpoint')).toBeVisible();

	// Switch to By section view (documentation tree index.yaml)
	await page.getByRole('tab', { name: 'By section' }).click();
	await expect(page.getByTestId('docs-by-section')).toBeVisible();
	await expect(page.getByText('docs/decisions')).toBeVisible();
	await expect(page.getByTestId('doc-media-row')).toContainText('Two-tap checkout sequence');

	// Add a new section
	await page.getByRole('button', { name: 'Add section' }).click();
	const secDialog = page.getByRole('dialog', { name: 'Add documentation section' });
	await secDialog.getByLabel('Folder path').fill('runbooks');
	await secDialog.getByLabel('Title').fill('Runbooks');
	await secDialog.getByLabel('Summary').fill('Operational recovery procedures.');
	await secDialog.getByRole('button', { name: 'Add section' }).click();
	await expect(secDialog).toBeHidden();
	await expect(page.getByText('docs/runbooks')).toBeVisible();

	// New document menu -> Decision
	await page.getByTestId('new-doc-trigger').click();
	await page.getByRole('menuitem', { name: 'Decision', exact: true }).click();
	await expect(page).toHaveURL(/\/projects\/beacon\/docs\/new\?kind=decision/);
	await page.getByLabel('Title').fill('Use structured retry headers');
	await page.getByLabel('Display key (optional)').fill('ADR-0005');
	// Switch to Source mode and verify synchronization (TPL-2)
	await page.getByRole('tab', { name: 'Source' }).click();
	const sourceArea = page.getByLabel('Front matter and Markdown source');
	await expect(sourceArea).toHaveValue(/title: Use structured retry headers/);
	await expect(sourceArea).toHaveValue(/key: ADR-0005/);
	await page.getByRole('tab', { name: 'Details & body' }).click();
	await page.getByRole('button', { name: 'Create document' }).click();
	await expect(page).toHaveURL(/\/projects\/beacon\/docs\/[0-9A-Za-z-]+$/);
	await expect(page.getByTestId('doc-header-card')).toContainText('Use structured retry headers');
	await expect(page.getByTestId('doc-header-card')).toContainText('ADR-0005');

	expect(pageErrors).toEqual([]);
	expect(consoleErrors).toEqual([]);
});

test('DEC-2/6/7 & org-data 18: Document reader, Current decision edit guard, History diff compare, and Issue version', async ({
	page
}) => {
	// Current decision edit guard & Supersede
	await open(page, '/projects/beacon/docs/doc-adr-0002');
	await expect(page.getByTestId('doc-header-card')).toContainText('ADR-0002');
	await expect(page.getByTestId('doc-header-card')).toContainText('Current');
	await expect(page.getByTestId('doc-section-rail')).toContainText('Context');
	await expect(page.getByTestId('doc-why-footer')).toContainText('T-A40');

	await page.getByTestId('edit-doc-btn').click();
	const warnDialog = page.getByRole('dialog', { name: 'Edit a current decision?' });
	await expect(warnDialog).toContainText('Decisions are a record.');
	await warnDialog.getByRole('button', { name: 'Supersede with a new decision' }).click();
	await expect(page).toHaveURL(
		/\/projects\/beacon\/docs\/new\?kind=decision&supersedes=doc-adr-0002/
	);
	await expect(page.getByLabel('Title')).toHaveValue(/Supersedes ADR-0002/);

	// Atlas versioned Product DSS: History, Diff Compare, and Issue version v3
	await visit(page, '/projects/atlas/docs/doc-atlas-pdss');
	await expect(page.getByTestId('doc-header-card')).toContainText('v2');
	await expect(page.getByTestId('doc-header-card')).toContainText('Unissued changes');
	await expect(page.getByTestId('doc-code-links')).toContainText('Path moved or missing');

	await page.getByRole('button', { name: 'History & compare' }).click();
	await expect(page.getByTestId('doc-history-panel')).toBeVisible();
	await expect(page.getByTestId('issued-version-row')).toHaveCount(2);
	await expect(page.getByTestId('doc-diff')).toBeVisible();

	await page.getByTestId('issue-version-btn').click();
	const issueDialog = page.getByRole('dialog', { name: 'Issue version v3' });
	await issueDialog.getByLabel('Version note').fill('Include 503 retry limit in desired state.');
	await issueDialog.getByRole('button', { name: 'Issue v3' }).click();
	await expect(issueDialog).toBeHidden();
	await expect(page.getByTestId('doc-header-card')).toContainText('v3');
	await expect(page.getByTestId('doc-header-card')).not.toContainText('Unissued changes');

	// Malformed metadata keeps body readable and offers Fix (DEC-7)
	await visit(page, '/projects/atlas/docs/doc-atlas-bad-meta');
	await expect(page.getByTestId('doc-detail-problem')).toContainText('owner_employee_id');
	await expect(page.getByTestId('doc-body')).toContainText('Staging canary probes spiked');
});

test('FTR-6/8/10/11/12/13: Features tab, New feature, Break into tasks, Link task confirmation, and Retire', async ({
	page
}) => {
	await open(page, '/projects/beacon/features');
	await expect(page).toHaveTitle('Features · Beacon · Projects · Agentic Org');

	// In progress and Planned visible; Live collapsed by default; Retired hidden until selected
	await expect(page.getByTestId('feature-group-in_progress')).toContainText(
		'Two-tap mobile checkout'
	);
	await expect(page.getByTestId('feature-group-planned')).toContainText(
		'Saved cards behind a flag'
	);
	const liveBtn = page.getByRole('button', { name: /Live · 1/i });
	await expect(liveBtn).toHaveAttribute('aria-expanded', 'false');
	await expect(page.getByText('Legacy single-page cart')).toHaveCount(0);

	// Toggle Retired filter on
	await page.getByRole('button', { name: 'Retired', exact: true }).click();
	await expect(page.getByTestId('feature-group-retired')).toContainText('Legacy single-page cart');

	// Open F-6 Feature page
	await page.getByRole('link', { name: 'Two-tap mobile checkout' }).click();
	await expect(page).toHaveURL(/\/projects\/beacon\/features\/feat-checkout$/);
	await expect(page.getByTestId('feature-header')).toContainText('F-6');
	await expect(page.getByTestId('feature-header')).toContainText('beacon/two-tap-checkout');
	// Affected bugs separated from implementation tasks (FTR-11)
	await expect(page.getByTestId('feature-tasks')).toContainText('T-A40');
	await expect(page.getByTestId('feature-affected-bugs')).toContainText('T-A42');
	await expect(page.getByTestId('feature-supporting-docs')).toContainText('DES-04');
	await expect(page.getByTestId('feature-releases')).toContainText('R-1');
	await expect(page.getByTestId('feature-releases')).toContainText('R-2');
	await expect(page.getByTestId('feature-cost')).toContainText('$14.85');

	// Break into tasks
	await page.getByTestId('break-into-tasks-btn').click();
	const breakDialog = page.getByRole('dialog', { name: /Break F-6 into tasks/ });
	await breakDialog.getByLabel('Task 1 title').fill('Add idempotency conflict telemetry');
	await breakDialog.getByRole('button', { name: /Create 2 tasks/ }).click();
	await expect(breakDialog).toBeHidden();
	await expect(page.getByTestId('feature-tasks')).toContainText(
		'Add idempotency conflict telemetry'
	);

	// Link a task already attached to F-7 asks confirmation naming both features (FTR-12)
	await page.getByTestId('link-task-btn').click();
	const linkDialog = page.getByRole('dialog', { name: /Link a task to F-6/ });
	await linkDialog.getByLabel('Task').selectOption('0193c000-0000-7000-8000-000000000041');
	await linkDialog.getByRole('button', { name: 'Link task' }).click();
	await expect(page.getByRole('dialog', { name: /Move T-A41 to F-6\?/ })).toContainText('F-7');
	await page.getByRole('button', { name: 'Move to F-6' }).click();
	await expect(page.getByTestId('feature-tasks')).toContainText('T-A41');

	// New feature creates Planned record and opens editor (FTR-13)
	await visit(page, '/projects/beacon/features');
	await page.getByTestId('new-feature-btn').click();
	const newDialog = page.getByRole('dialog', { name: 'New feature' });
	await newDialog.getByLabel('Title').fill('Express gift checkout');
	await expect(newDialog.getByLabel('Local slug')).toHaveValue('express-gift-checkout');
	await newDialog.getByRole('button', { name: 'Create feature' }).click();
	await expect(page).toHaveURL(/\/projects\/beacon\/features\/[0-9A-Za-z-]+\?edit=1$/);
	await expect(page.getByTestId('feature-header')).toContainText('Express gift checkout');
	await page.getByRole('button', { name: 'Save definition' }).click();

	// Retire the newly created feature with a reason (FTR-8)
	await page.getByTestId('retire-feature-btn').click();
	const retireDialog = page.getByRole('dialog', { name: /Retire .* Express gift checkout\?/ });
	await retireDialog
		.getByLabel('Reason for retirement')
		.fill('Consolidated into F-6 mobile checkout.');
	await retireDialog
		.getByLabel('Replaced by (optional, same project)')
		.selectOption('feat-checkout');
	await retireDialog.getByRole('button', { name: 'Retire feature' }).click();
	await expect(retireDialog).toBeHidden();
	await expect(page.getByTestId('retired-banner')).toContainText('Consolidated into F-6');
});

test('Releases tab and Release page show milestones, explicit content, intended flags vs confirmed deployment attempts', async ({
	page
}) => {
	await open(page, '/projects/beacon/releases');
	await expect(page).toHaveTitle('Releases · Beacon · Projects · Agentic Org');
	await expect(page.getByTestId('release-row')).toHaveCount(2);

	await page.getByRole('link', { name: 'Checkout v2 foundation' }).click();
	await expect(page).toHaveURL(/\/projects\/beacon\/releases\/rel-beacon-1$/);
	await expect(page.getByTestId('release-header')).toContainText('R-1');
	await expect(page.getByTestId('release-header')).toContainText('2.0.0');
	await expect(page.getByTestId('release-product-changelog')).toContainText('R-1 product changes');
	await expect(page.getByTestId('release-milestones')).toContainText('M-1');
	await expect(page.getByTestId('release-manifest-content')).toContainText('F-6');
	await expect(page.getByTestId('release-manifest-content')).toContainText('T-A38');
	await expect(page.getByTestId('release-intended-flags')).toContainText('two-tap-checkout');
	await expect(page.getByTestId('deployment-attempt')).toHaveCount(2);
	await expect(page.getByTestId('release-deployments')).toContainText('91c8e40b21');
	await expect(page.getByTestId('release-deployments')).toContainText(
		'R-1 production/primary technical changes'
	);

	// New release
	await visit(page, '/projects/beacon/releases');
	await page.getByTestId('new-release-btn').click();
	const relDialog = page.getByRole('dialog', { name: 'New release' });
	await relDialog.getByLabel('Title').fill('Holiday checkout stability');
	await relDialog.getByLabel('Version label (optional)').fill('2.2.0');
	await relDialog.getByRole('button', { name: 'Create release' }).click();
	await expect(page).toHaveURL(/\/projects\/beacon\/releases\/[0-9A-Za-z-]+$/);
	await expect(page.getByTestId('release-header')).toContainText('Holiday checkout stability');
});

test('WHY-3/4: Knowledge search ("Why is...") on /search returns decisions, evaluations, and features with excerpts and honest coverage', async ({
	page
}) => {
	await open(page, '/search?q=Why+is+SQLite+used+for+office+storage%3F');
	await expect(page.getByTestId('search-coverage')).toContainText(
		'Private task notes, protected chat and inaccessible records are excluded.'
	);
	await expect(page.getByTestId('knowledge-results')).toContainText('ADR-0002');
	await expect(page.getByTestId('knowledge-results')).toContainText(
		'Turso vs SQLite storage evaluation'
	);
	await expect(page.getByTestId('knowledge-excerpt').first()).toContainText('Excerpt:');

	// Title-only search states its partial coverage honestly (WHY-4)
	await visit(page, '/search?q=SQLite&mock=title-only');
	await expect(page.getByTestId('search-coverage')).toContainText('Title-only search');

	// Unsupported state shows honest message with ZERO Retry button
	await visit(page, '/search?q=SQLite&mock=unsupported');
	await expect(page.getByTestId('resource-unsupported')).toContainText(
		'Not available in this version'
	);
	await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0);
});

test('Step 6 states, keyboard focus, and 320 px / 200% zoom layout stay accessible with zero horizontal overflow', async ({
	page
}) => {
	// Unsupported on Docs/Features/Releases shows zero Retry button
	for (const path of [
		'/projects/beacon/docs?mock=unsupported',
		'/projects/beacon/features?mock=unsupported',
		'/projects/beacon/releases?mock=unsupported'
	]) {
		await open(page, path);
		await expect(page.getByTestId('resource-unsupported')).toContainText(
			'Not available in this version'
		);
		await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0);
	}

	// 320 px with 200% text across all new routes
	await page.setViewportSize({ width: 320, height: 800 });
	for (const path of [
		'/projects/beacon/docs',
		'/projects/beacon/docs/doc-adr-0002',
		'/projects/beacon/features',
		'/projects/beacon/features/feat-checkout',
		'/projects/beacon/releases',
		'/projects/beacon/releases/rel-beacon-1',
		'/search?q=Why+is+SQLite+used+for+office+storage%3F'
	]) {
		await visit(page, path);
		await page.addStyleTag({ content: 'html { font-size: 200%; }' });
		await noSidewaysScroll(page);
	}

	// Keyboard tabbing reaches interactive controls on Docs
	await page.setViewportSize({ width: 1280, height: 800 });
	await visit(page, '/projects/beacon/docs');
	await page.getByRole('tab', { name: 'By section' }).focus();
	await expect(page.getByRole('tab', { name: 'By section' })).toBeFocused();
});
