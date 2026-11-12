import { expect, test } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { viewAction } from './org-chart-menu';
import { create, iconIs, removeIs, focusTarget, team, showChart } from './non-chat-icons';

for (const width of [320, 640, 960, 1280]) for (const scale of [1, 1.25, 2]) {
	for (const screen of ['projects', 'profile', 'chart'] as const) {
		test(`E3 PROJ-2/3 TEAM-8 BUG-70: ${screen} uses non-text icons at ${width}px and ${scale * 100}% text`, async ({ page }) => {
			await create(page, width, scale);
			await visit(page, screen === 'profile' ? '/projects/beacon' : screen === 'chart' ? '/org-chart' : '/projects');
			if (screen === 'chart') await showChart(page);
			await expect(page.locator('html')).toHaveCSS('font-size', `${16 * scale}px`);
			if (screen === 'profile') {
				const breadcrumb = page.getByRole('navigation', { name: 'Breadcrumb', exact: true });
				await iconIs(breadcrumb.locator('svg'), 'mini:chevron-right');
				await expect(breadcrumb).toHaveText(/Projects\s*Beacon/);
				await expect(breadcrumb).not.toContainText('›');
				const projects = breadcrumb.getByRole('link', { name: 'Projects', exact: true });
				await expect(projects).toHaveAttribute('href', '/projects');
				expect(await breadcrumb.evaluate((element) => {
					const link = element.querySelector('a')!, icon = element.querySelector('svg')!, label = element.querySelector('span')!;
					return !!(link.compareDocumentPosition(icon) & Node.DOCUMENT_POSITION_FOLLOWING) && !!(icon.compareDocumentPosition(label) & Node.DOCUMENT_POSITION_FOLLOWING);
				})).toBe(true);
				await removeIs(page.getByRole('button', { name: 'Remove Beacon from Web', exact: true }), 'Remove Beacon from Web');
				await shot(page, `non-chat-profile-${width}-${scale}`, { context: page.getByTestId('project-name').locator('..') });
				await projects.focus();
				await projects.press('Enter');
				await expect(page).toHaveURL(/\/projects$/);
			} else if (screen === 'projects') {
				const row = page.locator('[data-testid=project-row][data-project=beacon]');
				await removeIs(row.getByRole('button', { name: 'Remove Beacon from Web', exact: true }), 'Remove Beacon from Web');
				await shot(page, `non-chat-projects-${width}-${scale}`, { context: row });
			} else {
				await expect(page.getByTestId('teams-chart')).toBeVisible();
				const remove = team(page, 'web').getByRole('button', { name: 'Remove Beacon from Web', exact: true });
				await iconIs(remove.locator('svg'), 'mini:x-mark');
				const spacing = await remove.evaluate((button) => {
					const box = button.getBoundingClientRect(), x = box.x + box.width / 2, y = box.y + box.height / 2;
					const others = [...button.closest('[data-testid=teams-chart]')!.querySelectorAll('button,a[href],input,select,textarea,[tabindex]:not([tabindex="-1"])')].filter((element) => element !== button && element.getClientRects().length);
					return others.map((element) => { const r = element.getBoundingClientRect(); return Math.hypot(Math.max(r.left - x, 0, x - r.right), Math.max(r.top - y, 0, y - r.bottom)); });
				});
				expect(spacing.length).toBeGreaterThan(0);
				expect(Math.min(...spacing), 'fitted remove centre clears other targets by 12 displayed px').toBeGreaterThanOrEqual(12);
				await focusTarget(page, remove);
				await shot(page, `non-chat-chart-fit-${width}-${scale}`, { context: remove });
				await viewAction(page, 'Actual size');
				await removeIs(remove, 'Remove Beacon from Web');
				await focusTarget(page, remove);
				await shot(page, `non-chat-chart-actual-${width}-${scale}`, { context: remove });
			}
		});
	}
}

for (const screen of ['projects', 'profile', 'chart'] as const) {
	test(`E3 PROJ-2/3 TEAM-8 BUG-70: ${screen} remove icons keep keyboard commands and last-team cancel`, async ({ page }) => {
		await create(page, 1280);
		await visit(page, screen === 'profile' ? '/projects/beacon' : screen === 'chart' ? '/org-chart' : '/projects');
		if (screen === 'chart') await viewAction(page, 'Actual size');
		const scope = screen === 'chart' ? team(page, 'web') : page.getByRole('main');
		const remove = scope.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await focusTarget(page, remove);
		await expect(remove).toHaveCSS('outline-style', 'solid');
		await shot(page, `non-chat-${screen}-remove-focused`, { context: remove });
		await remove.press('Enter');
		await expect(page.getByText('Removed Beacon from Web.', { exact: true })).toBeVisible();
		await expect(remove).toHaveCount(0);
		const last = page.getByRole('button', { name: 'Remove Beacon from Design', exact: true });
		await last.focus();
		await last.press('Enter');
		const dialog = page.getByRole('dialog', { name: 'Remove Beacon from Design?', exact: true });
		await expect(dialog).toContainText('It will have no team and show under Unassigned projects.');
		await shot(page, `non-chat-${screen}-last-team`, { context: dialog });
		await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
		await expect(dialog).toBeHidden();
		await expect(last).toBeFocused();
		await removeIs(last, 'Remove Beacon from Design');
	});
}

for (const [screen, path, empty, emptyId] of [
	['projects', '/projects', '/projects?org=empty', 'no-projects'],
	['profile', '/projects/beacon', '/projects/ghost', 'project-gone'],
	['chart', '/org-chart', '/org-chart?org=empty', '']
] as const) {
	test(`E3 PROJ-1/3 TEAM-8 BUG-70: ${screen} preserves empty, loading, error and recovered states`, async ({ page }) => {
		await create(page, 320, 2);
		await visit(page, empty);
		if (emptyId) await expect(page.getByTestId(emptyId)).toBeVisible();
		else await expect(page.getByText('No teams yet', { exact: true })).toBeVisible();
		await expect(page.getByRole('button', { name: /^Remove Beacon from/ })).toHaveCount(0);
		await shot(page, `non-chat-${screen}-empty`);
		const response = await page.request.get('/e2e/fakes/org-api.ts');
		expect(response.ok()).toBe(true);
		const module = await response.text();
		let entered!: () => void, release!: () => void;
		const started = new Promise<void>((resolve) => { entered = resolve; });
		const held = new Promise<void>((resolve) => { release = resolve; });
		await page.exposeFunction('__nonChatEntered', entered);
		await page.exposeFunction('__nonChatRelease', () => held);
		await page.route('**/e2e/fakes/org-api.ts*', (route) => route.fulfill({ contentType: 'text/javascript', body: module + '\norgApi.view = async () => { await window.__nonChatEntered(); await window.__nonChatRelease(); throw { source:"api", code:"storage_error", message:"Pseudo-icon office unavailable" }; };' }));
		try {
			await page.goto(path);
			await started;
			await expect(page.getByRole('button', { name: /^Remove Beacon from/ })).toHaveCount(0);
			await expect(page.getByText('Pseudo-icon office unavailable', { exact: true })).toHaveCount(0);
			await shot(page, `non-chat-${screen}-loading`, { settle: false });
			release();
			await expect(page.getByText('Pseudo-icon office unavailable', { exact: true })).toBeVisible();
			await expect(page.getByRole('button', { name: /^Remove Beacon from/ })).toHaveCount(0);
			await shot(page, `non-chat-${screen}-error`);
		} finally {
			release();
			await page.unrouteAll({ behavior: 'wait' });
		}
		await page.reload();
		if (screen === 'chart') await showChart(page);
		await expect(page.getByRole('button', { name: 'Remove Beacon from Web', exact: true })).toBeVisible();
		await shot(page, `non-chat-${screen}-recovered`);
	});
}

for (const screen of ['projects', 'chart'] as const) {
	test(`E3 PROJ-2 TEAM-8 BUG-70: ${screen} remove icon inherits the button hover colour`, async ({ page }) => {
		await create(page, 1280);
		await visit(page, screen === 'chart' ? '/org-chart' : '/projects');
		if (screen === 'chart') await viewAction(page, 'Actual size');
		const remove = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await remove.hover();
		await expect(remove).toHaveCSS('color', 'rgb(255, 255, 255)');
		await expect(remove.locator('svg')).toHaveCSS('color', 'rgb(255, 255, 255)');
		await shot(page, `non-chat-${screen}-remove-hover`, { context: remove });
	});
}
