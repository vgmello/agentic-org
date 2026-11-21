import { readFile } from 'node:fs/promises';
import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { signInAsAdmin } from './web-session';
import { enterMockedOrganization } from './mock-organization';

// Step 7 (TASK-69): Project Insights (/projects/[id]/insights), Organization Insights (/insights),
// and Member profile Cost tab (/members/[memberId]/cost) against the mock costs API.
async function openOrganization(page: Page) {
	await signInAsAdmin(page, [
		{
			id: 'org-insights',
			name: 'Lumen Labs',
			created_at: '2026-11-18T00:00:00Z',
			recovery: 'acknowledged'
		}
	]);
	await enterMockedOrganization(page, 'Lumen Labs');
}

async function noSidewaysScroll(page: Page) {
	const overflow = await page.evaluate(
		() => document.documentElement.scrollWidth - document.documentElement.clientWidth
	);
	expect(overflow).toBeLessThanOrEqual(0);
}

test('INS-1..5: Project Insights shows investment lead, KPI tiles, trend chart/table, breakdowns, release caps, and CSV export', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/projects/beacon/insights');
	await expect(page).toHaveTitle('Insights · Beacon · Projects · Agentic Org');

	const root = page.getByTestId('project-insights');
	await expect(root).toBeVisible();

	// Investment lead banner (decision 6, INS-5) with honest working time not tracked yet
	await expect(page.getByTestId('project-investment-lead')).toContainText('Beacon has cost ≈ $');
	await expect(page.getByTestId('project-investment-lead')).toContainText(
		'agent work: — (not tracked yet)'
	);

	// 4 KPI tiles (Cost, Tokens, Working time — not tracked yet, Done)
	const tiles = page.getByTestId('project-kpi-tiles');
	await expect(tiles).toContainText('≈ $24.90');
	await expect(tiles).toContainText('Working time');
	await expect(tiles).toContainText('not tracked yet');

	// Insights list with actionable links
	const insightsList = page.getByTestId('insights-list');
	await expect(insightsList).toBeVisible();
	await expect(insightsList.getByRole('link').first()).toBeVisible();

	// Spend trend chart + accessible Chart/Table toggle
	const trend = page.getByTestId('spend-trend');
	await expect(trend.getByTestId('trend-svg')).toBeVisible();
	await trend.getByRole('button', { name: 'Table', exact: true }).click();
	await expect(trend.getByTestId('trend-table')).toBeVisible();
	await trend.getByRole('button', { name: 'Chart', exact: true }).click();
	await expect(trend.getByTestId('trend-svg')).toBeVisible();

	// Breakdowns: switch tabs with keyboard and click, filter, sort, and open UsageRecordsSheet
	const bd = page.getByTestId('project-breakdowns');
	const tabs = bd.getByRole('tab');
	await expect(tabs).toHaveText(['Team', 'Consumer', 'Model', 'Task type', 'Work type']);
	await tabs.filter({ hasText: 'Team' }).focus();
	await page.keyboard.press('ArrowRight');
	await expect(tabs.filter({ hasText: 'Consumer' })).toBeFocused();
	await expect(tabs.filter({ hasText: 'Consumer' })).toHaveAttribute('aria-selected', 'true');

	await tabs.filter({ hasText: 'Model' }).click();
	await expect(bd.getByTestId('project-breakdown-table')).toBeVisible();
	await bd.getByTestId('breakdown-filter').fill('sonnet');
	await expect(bd.locator('[data-breakdown-row]:visible')).toHaveCount(1);
	await bd.getByTestId('breakdown-filter').fill('');

	// Sort by label ascending
	const modelColHeader = bd.getByRole('columnheader', { name: /Model/ });
	await modelColHeader.getByRole('button').click();
	await expect(modelColHeader).toHaveAttribute('aria-sort', 'ascending');

	// Drill down into effective usage records behind a breakdown row
	const costBtn = bd
		.locator('[data-breakdown-row]:visible')
		.first()
		.getByRole('button', { name: /Open the usage records/ });
	await costBtn.click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByTestId('records-sheet')).toBeVisible();
	await expect(sheet.locator('[data-record]').first()).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(sheet).toBeHidden();
	await expect(costBtn).toBeFocused();

	// Release caps section (neutral styling, shows Spent at release for released R-1)
	const releaseCaps = page.getByTestId('release-caps');
	await expect(releaseCaps).toBeVisible();
	await expect(releaseCaps).toContainText('Spent at release');

	// Speed medians section
	await expect(page.getByTestId('speed-summary')).toContainText('not tracked yet');

	// Safe CSV export triggers download with expected headers & effective records
	const [download] = await Promise.all([
		page.waitForEvent('download'),
		page.getByTestId('export-csv-btn').click()
	]);
	expect(download.suggestedFilename()).toBe('beacon-costs-this_month.csv');
	const csvPath = await download.path();
	expect(csvPath).toBeTruthy();
	const csvText = await readFile(csvPath!, 'utf8');
	expect(csvText.split('\r\n')[0]).toContain('event_id,occurred_at,consumer_kind');
	expect(csvText).toContain('beacon');
});

test('INS-5 & scope: Organization Insights (/insights) sorts, filters, drills into project insights, and enforces PM/viewer scope', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/insights');
	await expect(page).toHaveTitle('Insights · Agentic Org');
	await expect(page.getByTestId('organization-insights')).toBeVisible();

	const tiles = page.getByTestId('org-kpi-tiles');
	await expect(tiles).toContainText('≈ $123.90');
	await expect(tiles).toContainText('not tracked yet');

	// Projects table shows projects and org Not on a task row for owner
	const table = page.getByTestId('org-projects-table');
	await expect(table).toBeVisible();
	await expect(table.locator('[data-org-project="atlas"]')).toContainText('Atlas');
	await expect(table.locator('[data-org-project="beacon"]')).toContainText('Beacon');
	await expect(table.locator('[data-org-project="not-on-a-task"]')).toContainText('Not on a task');

	// Filter projects by name
	await page.getByTestId('org-projects-filter').fill('Beacon');
	await expect(table.locator('[data-org-project="beacon"]')).toBeVisible();
	await expect(table.locator('[data-org-project="atlas"]')).toHaveCount(0);
	await page.getByTestId('org-projects-filter').fill('');

	// Sort projects by name
	const projHeader = table.getByRole('columnheader', { name: /Project/ });
	await projHeader.getByRole('button').click();
	await expect(projHeader).toHaveAttribute('aria-sort', 'ascending');

	// Drill down into Project Insights
	await table.locator('[data-org-project="atlas"]').getByRole('link', { name: 'Atlas' }).click();
	await expect(page).toHaveURL(/\/projects\/atlas\/insights$/);
	await expect(page.getByTestId('project-investment-lead')).toContainText('Atlas has cost');

	// Project manager scope (?mock=manager): sees only managed project (Beacon), no org Not on a task
	await visit(page, '/insights?mock=manager');
	await expect(page.getByTestId('period-range')).toContainText('in your projects');
	const pmTable = page.getByTestId('org-projects-table');
	await expect(pmTable.locator('[data-org-project="beacon"]')).toBeVisible();
	await expect(pmTable.locator('[data-org-project="atlas"]')).toHaveCount(0);
	await expect(pmTable.locator('[data-org-project="not-on-a-task"]')).toHaveCount(0);

	// Non-authorized viewer (?mock=viewer): no amounts shown
	await visit(page, '/insights?mock=viewer');
	await expect(page.getByTestId('org-insights-not-authorized')).toContainText(
		"You don't see costs here"
	);
	await expect(page.locator('main')).not.toContainText('$');
});

test('COST-7 & BUD-1..4: Member profile Cost tab shows spend, owner budgets, PM scope, empty/error states, and person guard', async ({
	page
}) => {
	await openOrganization(page);

	// 1. Owner viewing Sam: period tiles, daily & monthly budgets, by project, top tasks, records drill-down
	await visit(page, '/members/sam/cost');
	const tab = page.getByTestId('member-cost-tab');
	await expect(tab).toBeVisible();
	await expect(tab).toHaveAttribute('data-scope', 'organization');
	await expect(page.getByTestId('member-period-tiles')).toContainText('Today');
	await expect(page.getByTestId('member-period-tiles')).toContainText('This month');
	await expect(page.getByTestId('member-period-tiles')).toContainText('≈ $51.02');

	const budgets = page.getByTestId('member-budgets-section');
	await expect(budgets).toBeVisible();
	await expect(budgets.locator('[data-member-budget="daily"]')).toContainText(
		'Today (America/New_York)'
	);
	await expect(budgets.locator('[data-member-budget="daily"]')).toContainText('80% reached');
	await expect(budgets.locator('[data-member-budget="monthly"]')).toContainText(
		'This month (America/New_York)'
	);

	// By project & top tasks open UsageRecordsSheet
	const byProject = page.getByTestId('member-by-project');
	await expect(byProject).toContainText('Atlas');
	const projBtn = byProject
		.locator('[data-member-project]')
		.first()
		.getByRole('button', { name: /Open records/ });
	await projBtn.click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.locator('[data-record]').first()).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(sheet).toBeHidden();
	await expect(projBtn).toBeFocused();

	// 2. Owner viewing Pax (no budget set) -> Set budget dialog validation & save
	await visit(page, '/members/pax/cost');
	await expect(page.getByTestId('member-no-budget')).toContainText('No budget set');
	await page.getByRole('button', { name: 'Set budget…' }).click();
	const dialog = page.getByRole('dialog');
	await dialog.getByRole('button', { name: 'Set budget' }).click();
	await expect(dialog).toContainText('Enter a budget amount.');
	await dialog.getByLabel('Limit (USD)').fill('15');
	await dialog.getByRole('button', { name: 'Set budget' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.locator('[data-member-budget="daily"]')).toContainText('$15.00');

	// 3. Owner viewing Kai (held + already extended): Paused · Budget reached, no Allow 20% once
	await visit(page, '/members/kai/cost');
	const kaiMonthly = page.locator('[data-member-budget="monthly"]');
	await expect(kaiMonthly).toContainText('Paused · Budget reached');
	await expect(kaiMonthly).toContainText('already used its one 20% extension');
	await expect(kaiMonthly.getByRole('button', { name: 'Allow 20% more once' })).toHaveCount(0);
	await expect(kaiMonthly.getByRole('button', { name: 'Raise budget…' })).toBeVisible();

	// 4. Owner viewing Mira (stop_pending) + neutral budget reset notice
	await visit(page, '/members/mira/cost?mock=budget-reset');
	await expect(page.getByTestId('budget-reset-note')).toContainText('Budget reset');
	await expect(page.locator('[data-member-budget="daily"]')).toContainText(
		'Stopping · Budget reached'
	);

	// 5. Project manager (?mock=manager, lead of Web/Beacon):
	//    - On Mira (has Beacon spend): labelled "in your projects", NO budget section, NO org Not on a task, only Beacon
	await visit(page, '/members/mira/cost?mock=manager');
	await expect(page.getByTestId('member-cost-tab')).toHaveAttribute('data-scope', 'managed');
	await expect(page.getByTestId('member-cost-tab')).toContainText('in your projects');
	await expect(page.getByTestId('member-budgets-section')).toHaveCount(0);
	await expect(page.getByTestId('member-by-project')).toContainText('Beacon');
	await expect(page.getByTestId('member-by-project')).not.toContainText('Atlas');
	await expect(page.getByTestId('member-by-project')).not.toContainText(
		'Not on a task (organization)'
	);

	//    - On Sam (only Atlas/org spend, none in Beacon): shows "No spend in your projects yet" and no budgets
	await visit(page, '/members/sam/cost?mock=manager');
	await expect(page.getByRole('heading', { name: 'No spend in your projects yet' })).toBeVisible();
	await expect(page.getByTestId('member-budgets-section')).toHaveCount(0);

	// 6. Empty (owner), error, unauthorized, and person guard states
	await visit(page, '/members/sam/cost?mock=empty');
	await expect(page.getByRole('heading', { name: 'No spend yet' })).toBeVisible();

	await visit(page, '/members/sam/cost?mock=costs-fail');
	await expect(
		page.getByRole('heading', { name: "Couldn't load costs. Try again." })
	).toBeVisible();
	await expect(page.getByRole('button', { name: 'Try again' })).toBeVisible();

	await visit(page, '/members/sam/cost?mock=viewer');
	await expect(page.getByRole('heading', { name: "You don't see costs here" })).toBeVisible();
	await expect(page.locator('main')).not.toContainText('$');

	// Person profile guard (/members/avery/cost)
	await visit(page, '/members/avery/cost');
	await expect(page.getByRole('heading', { name: "People don't have cost" })).toBeVisible();
});

test('Responsive: Project Insights, Organization Insights, and Member Cost reflow at 320 px and 200% text', async ({
	page
}) => {
	await openOrganization(page);
	await page.setViewportSize({ width: 320, height: 800 });

	for (const url of ['/projects/beacon/insights', '/insights', '/members/sam/cost']) {
		await visit(page, url);
		await noSidewaysScroll(page);
		await page.evaluate(() => (document.documentElement.style.fontSize = '200%'));
		await noSidewaysScroll(page);
		await page.evaluate(() => (document.documentElement.style.fontSize = ''));
	}
});
