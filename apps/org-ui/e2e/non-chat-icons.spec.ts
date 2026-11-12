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

for (const [screen, path, empty, emptyText] of [
	['projects', '/projects', '/projects?org=empty', 'No projects yet'],
	['profile', '/projects/beacon', '/projects/ghost', "This project isn't here any more."],
	['chart', '/org-chart', '/org-chart?org=empty', 'No teams yet']
] as const) {
	test(`E3 PROJ-1/3 TEAM-8 BUG-70: ${screen} preserves empty, loading, error and recovered states`, async ({ page }) => {
		await create(page, 320, 2);
		await visit(page, empty);
		const emptyState = page.getByText(emptyText, { exact: true });
		await expect(emptyState).toBeVisible();
		await emptyState.scrollIntoViewIfNeeded();
		await expect(page.getByRole('button', { name: /^Remove Beacon from/ })).toHaveCount(0);
		await shot(page, `non-chat-${screen}-empty`, { context: emptyState });
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
			const failure = page.getByText('Pseudo-icon office unavailable', { exact: true });
			await expect(failure).toBeVisible();
			await failure.scrollIntoViewIfNeeded();
			await expect(page.getByRole('button', { name: /^Remove Beacon from/ })).toHaveCount(0);
			await shot(page, `non-chat-${screen}-error`, { context: failure });
		} finally {
			release();
			await page.unrouteAll({ behavior: 'wait' });
		}
		await page.reload();
		if (screen === 'chart') await showChart(page);
		const recovered = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await expect(recovered).toBeVisible();
		await focusTarget(page, recovered);
		await shot(page, `non-chat-${screen}-recovered`, { context: recovered });
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

// BUG-114/115/117: exercise the displayed targets and native focus after the keyed DOM refresh.
for (const width of [320, 640, 960, 1280]) for (const root of [16, 32, 40]) {
	test(`TEAM-8 BUG-70 BUG-114 BUG-117: Fit removal target and ring at ${width}px root${root}`, async ({ page }) => {
		const errors: string[] = [];
		page.on('pageerror', (error) => errors.push(error.message));
		await create(page, width, root / 16);
		await visit(page, '/org-chart');
		await showChart(page);
		const chart = page.getByTestId('teams-chart');
		const remove = team(page, 'web').getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await geometryFocus(page, remove);
		await shot(page, `bug114-fit-${width}-${root}`, { context: remove });
		await removalRing(remove);
		const samples = await chart.evaluate(async (element) => {
			const sample = () => ({ scale: new DOMMatrix(getComputedStyle(element).transform).a,
				width: element.getBoundingClientRect().width, naturalWidth: (element as HTMLElement).offsetWidth });
			const values = [sample()];
			for (let i = 0; i < 2; i++) { await new Promise(requestAnimationFrame); values.push(sample()); }
			return values;
		});
		console.log(`BUG114-FIT ${JSON.stringify({ width, root, samples, target: await remove.boundingBox() })}`);
		expect(Math.max(...samples.map((x) => x.scale)) - Math.min(...samples.map((x) => x.scale))).toBeLessThanOrEqual(0.001);
		expect(errors.filter((error) => /ResizeObserver loop/.test(error))).toEqual([]);
		const box = (await remove.boundingBox())!;
		await page.getByRole('button', { name: 'View', exact: true }).focus();
		await page.keyboard.press('Enter');
		const actual = page.getByRole('menuitem', { name: 'Actual size', exact: true });
		await actual.focus();
		await actual.press('Enter');
		await geometryFocus(page, remove);
		await shot(page, `bug114-actual-${width}-${root}`, { context: remove });
		await expect(chart).toHaveCSS('transform', 'matrix(1, 0, 0, 1, 0, 0)');
		await removeIs(remove, 'Remove Beacon from Web');
		expect((await remove.boundingBox())!.width).toBeCloseTo(root * 1.5, 1);
		expect(box.width, 'counter-scaled displayed width').toBeGreaterThanOrEqual(root * 1.5 - 0.1);
		expect(box.height, 'counter-scaled displayed height').toBeGreaterThanOrEqual(root * 1.5 - 0.1);
		await removalRing(remove);
	});
}

async function removalRing(button: import('@playwright/test').Locator) {
	await expect(button).toHaveCSS('outline-offset', '2px');
	await expect(button).toHaveCSS('outline-style', 'solid');
	const clearances = await button.evaluate((element) => {
		const r = element.getBoundingClientRect(), values: { ancestor: string; sides: number[] }[] = [];
		for (let parent = element.parentElement; parent; parent = parent.parentElement) {
			const css = getComputedStyle(parent), box = parent.getBoundingClientRect();
			const x = /auto|scroll|hidden|clip/.test(css.overflowX), y = /auto|scroll|hidden|clip/.test(css.overflowY);
			if (x || y) values.push({ ancestor: `${parent.tagName}.${parent.className}`, sides: [
				...(x ? [r.left - box.left, box.right - r.right] : []), ...(y ? [r.top - box.top, box.bottom - r.bottom] : [])] });
		}
		return values;
	});
	for (const clip of clearances) for (const side of clip.sides) expect(side, clip.ancestor).toBeGreaterThanOrEqual(4 - 0.1);
}

async function chipFixture(page: import('@playwright/test').Page) {
	await page.evaluate(async () => {
		const connections = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]');
		const id = connections[connections.length - 1].organization.id;
		const modulePath = '/e2e/fakes/org-api.ts';
		const { orgApi } = await import(modulePath);
		const { snapshot } = await orgApi.view(id);
		const links = [['beacon', 'platform'], ['atlas', 'web'], ['scratch', 'web'], ['scratch', 'design']];
		localStorage.setItem('agentic-org.mock.org.' + id, JSON.stringify({ snapshot: { ...snapshot,
			project_teams: [...snapshot.project_teams, ...links.map(([project_id, team_id], index) => ({ id: `chip-fixture-${index}`, project_id, team_id, version: 1 }))]
		}, answers: {}, order: [] }));
	});
}

for (const screen of ['projects', 'profile', 'chart'] as const) {
	for (const direction of ['next', 'previous', 'middle'] as const) {
		test(`TEAM-8 BUG-70 BUG-115: ${screen} unlink focuses keyed ${direction} in its own group`, async ({ page }) => {
			await create(page, 1280);
			await chipFixture(page);
			await visit(page, screen === 'profile' ? '/projects/beacon' : screen === 'chart' ? '/org-chart' : '/projects');
			const name = screen === 'chart' ? `Remove ${direction === 'next' ? 'Atlas' : direction === 'previous' ? 'Scratchpad' : 'Beacon'} from Web` : `Remove Beacon from ${direction === 'next' ? 'Design' : direction === 'previous' ? 'Web' : 'Platform'}`;
			const remove = page.getByRole('button', { name, exact: true });
			await focusTarget(page, remove);
			await remove.press('Enter');
			await expect(remove).toHaveCount(0);
			const target = page.getByRole('button', { name: screen === 'chart' ? `Remove ${direction === 'middle' ? 'Scratchpad' : 'Beacon'} from Web` : `Remove Beacon from ${direction === 'middle' ? 'Web' : 'Platform'}`, exact: true });
			await expect(target).toBeFocused();
			await shot(page, `bug115-${screen}-${direction}`, { context: target });
		});
	}
	test(`TEAM-8 BUG-70 BUG-115: ${screen} immediate and confirmed-last unlink keep local fallback`, async ({ page }) => {
		await create(page, 1280);
		await visit(page, screen === 'profile' ? '/projects/beacon' : screen === 'chart' ? '/org-chart' : '/projects');
		const remove = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await focusTarget(page, remove);
		await remove.press('Enter');
		await expect(remove).toHaveCount(0);
		const last = page.getByRole('button', { name: 'Remove Beacon from Design', exact: true });
		await expect(screen === 'chart' ? page.getByRole('button', { name: 'Actions for Web', exact: true }) : last).toBeFocused();
		await last.focus();
		await last.press('Enter');
		const dialog = page.getByRole('dialog', { name: 'Remove Beacon from Design?', exact: true });
		await expect(dialog).toBeVisible();
		await dialog.getByRole('button', { name: 'Remove', exact: true }).click();
		await expect(dialog).toBeHidden();
		await expect(last).toHaveCount(0);
		const target = page.getByRole('button', { name: screen === 'chart' ? 'Actions for Design' : 'Assign a team to Beacon', exact: true });
		await expect(target).toBeFocused();
		await shot(page, `bug115-${screen}-last-success`, { context: target });
	});
	test(`TEAM-8 BUG-70 BUG-115: ${screen} refusal, cancel and Escape preserve the opening remove`, async ({ page }) => {
		await create(page, 1280);
		await visit(page, screen === 'profile' ? '/projects/beacon' : screen === 'chart' ? '/org-chart' : '/projects');
		const remove = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await page.evaluate(() => (window as unknown as { __mockOrgFailNext(method: string): void }).__mockOrgFailNext('project.remove_team'));
		await focusTarget(page, remove);
		await remove.press('Enter');
		await expect(page.getByText(/Couldn't remove Beacon from Web:/)).toBeVisible();
		await shot(page, `bug115-${screen}-immediate-refused`, { context: remove });
		await expect(remove).toBeFocused();
		await remove.press('Enter');
		await expect(remove).toHaveCount(0);
		const last = page.getByRole('button', { name: 'Remove Beacon from Design', exact: true });
		const dialog = page.getByRole('dialog', { name: 'Remove Beacon from Design?', exact: true });
		for (const close of ['cancel', 'escape', 'refusal'] as const) {
			await last.focus();
			await last.press('Enter');
			await expect(dialog).toBeVisible();
			if (close === 'cancel') await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
			else if (close === 'escape') await page.keyboard.press('Escape');
			else {
				await page.evaluate(() => (window as unknown as { __mockOrgFailNext(method: string): void }).__mockOrgFailNext('project.remove_team'));
				await dialog.getByRole('button', { name: 'Remove', exact: true }).click();
				await expect(dialog.getByRole('alert')).toHaveText('The office went away.');
				expect(await dialog.evaluate((element) => element.contains(document.activeElement))).toBe(true);
				await shot(page, `bug115-${screen}-refused`, { context: dialog });
				await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
			}
			await expect(dialog).toBeHidden();
			await expect(last).toBeFocused();
		}
	});
}

async function holdUnlink(page: import('@playwright/test').Page) {
	await page.evaluate(async () => {
		const modulePath = '/e2e/fakes/org-api.ts';
		const { orgApi } = await import(modulePath) as { orgApi: import('../src/lib/org/api').OrgApi };
		const original = orgApi.command;
		let release!: () => void;
		const held = new Promise<void>((resolve) => { release = resolve; });
		const hooks = window as unknown as { __chipHold: { entered: boolean; release(): void } };
		hooks.__chipHold = { entered: false, release };
		orgApi.command = async (id, input) => {
			if (input.method === 'project.remove_team') { hooks.__chipHold.entered = true; await held; }
			return original(id, input);
		};
	});
}
async function releaseUnlink(page: import('@playwright/test').Page) {
	await page.evaluate(() => (window as unknown as { __chipHold: { release(): void } }).__chipHold.release());
}

for (const screen of ['projects', 'profile', 'chart'] as const) {
	test(`TEAM-8 BUG-70 BUG-115: ${screen} pending unlink does not steal moved-away focus`, async ({ page }) => {
		await create(page, 1280);
		await visit(page, screen === 'profile' ? '/projects/beacon' : screen === 'chart' ? '/org-chart' : '/projects');
		await holdUnlink(page);
		const remove = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await focusTarget(page, remove);
		await remove.press('Enter');
		await page.waitForFunction(() => (window as unknown as { __chipHold: { entered: boolean } }).__chipHold.entered);
		const away = page.getByRole('button', { name: screen === 'chart' ? 'Actions for Engineering' : 'Assign a team to Beacon', exact: true });
		await away.focus();
		await expect(away).toBeFocused();
		await releaseUnlink(page);
		await expect(remove).toHaveCount(0);
		await expect(away).toBeFocused();
		await shot(page, `bug115-${screen}-no-steal`, { context: away });
	});
	test(`TEAM-8 BUG-70 BUG-115: ${screen} resolves the next key after a live reorder`, async ({ page }) => {
		await create(page, 1280);
		await chipFixture(page);
		await visit(page, screen === 'profile' ? '/projects/beacon' : screen === 'chart' ? '/org-chart' : '/projects');
		await holdUnlink(page);
		const remove = page.getByRole('button', { name: screen === 'chart' ? 'Remove Atlas from Web' : 'Remove Beacon from Design', exact: true });
		await focusTarget(page, remove);
		await remove.press('Enter');
		await page.waitForFunction(() => (window as unknown as { __chipHold: { entered: boolean } }).__chipHold.entered);
		await page.evaluate(async (screen) => {
			const connections = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]');
			const id = connections[connections.length - 1].organization.id;
			const hooks = window as unknown as { __mockOrgChange(id: string, command: object): Promise<unknown> };
			await hooks.__mockOrgChange(id, screen === 'chart'
				? { method: 'project.update', params: { project_id: 'beacon', expected_version: 1, name: 'Zulu' } }
				: { method: 'team.rename', params: { team_id: 'platform', expected_version: 1, name: 'Zulu' } });
		}, screen);
		const target = page.getByRole('button', { name: screen === 'chart' ? 'Remove Zulu from Web' : 'Remove Beacon from Zulu', exact: true });
		await expect(target).toBeVisible();
		await expect(remove).toBeFocused();
		await releaseUnlink(page);
		await expect(remove).toHaveCount(0);
		await expect(target).toBeFocused();
		await shot(page, `bug115-${screen}-keyed-reorder`, { context: target });
	});
}

for (const screen of ['projects', 'profile'] as const) {
	test(`PROJ-2/3 BUG-70 BUG-117: ${screen} removal ring clears clipped ancestors`, async ({ page }) => {
		await create(page, 320, 2);
		await visit(page, screen === 'profile' ? '/projects/beacon' : '/projects');
		const remove = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await focusTarget(page, remove);
		await shot(page, `bug117-${screen}-ring`, { context: remove });
		await removalRing(remove);
	});
}

async function geometryFocus(page: import('@playwright/test').Page, button: import('@playwright/test').Locator) {
	await page.keyboard.press('Tab');
	await button.evaluate((element) => element.scrollIntoView({ block: 'center', inline: 'center' }));
	await button.focus();
	await expect(button).toBeFocused();
}

for (const direction of ['next', 'previous'] as const) {
	test(`TEAM-8 BUG-70 BUG-115: chart confirmed-last unlink uses the same keyed ${direction} resolver`, async ({ page }) => {
		await create(page, 1280);
		await chipFixture(page);
		await page.evaluate(async (direction) => {
			const connections = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]');
			const id = connections[connections.length - 1].organization.id;
			const hooks = window as unknown as { __mockOrgChange(id: string, command: object): Promise<unknown> };
			await hooks.__mockOrgChange(id, { method: 'project.remove_team', params: direction === 'next'
				? { project_team_id: 'pt-1', expected_version: 1, areas_to_team_id: 'web' }
				: { project_team_id: 'chip-fixture-3', expected_version: 1 } });
		}, direction);
		await visit(page, '/org-chart');
		const name = `Remove ${direction === 'next' ? 'Atlas' : 'Scratchpad'} from Web`;
		const remove = page.getByRole('button', { name, exact: true });
		await focusTarget(page, remove);
		await remove.press('Enter');
		const dialog = page.getByRole('dialog', { name: `${name}?`, exact: true });
		await expect(dialog).toBeVisible();
		await dialog.getByRole('button', { name: 'Remove', exact: true }).click();
		await expect(dialog).toBeHidden();
		await expect(remove).toHaveCount(0);
		const target = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await expect(target).toBeFocused();
		await shot(page, `bug115-chart-confirmed-${direction}`, { context: target });
	});
}

test('TEAM-8 BUG-70 BUG-115: chart falls back to its region if the source card disappears during success', async ({ page }) => {
	await create(page, 1280);
	await visit(page, '/org-chart');
	await page.evaluate(async () => {
		const modulePath = '/e2e/fakes/org-api.ts';
		const { orgApi } = await import(modulePath) as { orgApi: import('../src/lib/org/api').OrgApi };
		const original = orgApi.command;
		orgApi.command = async (id, input) => {
			const result = await original(id, input);
			if (input.method === 'project.remove_team') await original(id, { method: 'team.delete', params: { team_id: 'web', expected_version: 1 } });
			return result;
		};
	});
	const remove = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
	await focusTarget(page, remove);
	await remove.press('Enter');
	await expect(team(page, 'web')).toHaveCount(0);
	const region = page.getByRole('region', { name: 'Teams chart', exact: true });
	await expect(region).toBeFocused();
	await expect(page.getByRole('button', { name: 'Remove Beacon from Design', exact: true })).not.toBeFocused();
	await shot(page, 'bug115-chart-region-fallback', { context: region });
});

for (const root of [16, 32, 40]) {
	test(`TEAM-8 BUG-70 BUG-114: Actual size keeps the unfocused control at root${root}`, async ({ page }) => {
		await create(page, 1280, root / 16);
		await visit(page, '/org-chart');
		await page.getByRole('button', { name: 'View', exact: true }).focus();
		await page.keyboard.press('Enter');
		const actual = page.getByRole('menuitem', { name: 'Actual size', exact: true });
		await actual.focus();
		await actual.press('Enter');
		await expect(page.getByTestId('teams-chart')).toHaveCSS('transform', 'matrix(1, 0, 0, 1, 0, 0)');
		await page.evaluate(async () => { await new Promise(requestAnimationFrame); await new Promise(requestAnimationFrame); });
		const remove = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await removeIs(remove, 'Remove Beacon from Web');
		await expect(remove).not.toBeFocused();
		await shot(page, `bug114-actual-unfocused-${root}`, { context: remove });
	});
}

for (const screen of ['projects', 'profile', 'chart'] as const) {
	test(`TEAM-8 BUG-70 BUG-115: ${screen} successful stale refresh can still transfer focus from its retained chip`, async ({ page }) => {
		await create(page, 1280);
		await visit(page, screen === 'profile' ? '/projects/beacon' : screen === 'chart' ? '/org-chart' : '/projects');
		await page.evaluate(async () => {
			const connections = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]');
			const id = connections[connections.length - 1].organization.id;
			const modulePath = '/e2e/fakes/org-api.ts';
			const { orgApi } = await import(modulePath) as { orgApi: import('../src/lib/org/api').OrgApi };
			const command = orgApi.command, view = orgApi.view, stale = await view(id);
			orgApi.command = async (organizationId, input) => {
				if (input.method === 'project.remove_team') orgApi.view = async () => structuredClone(stale);
				return command(organizationId, input);
			};
			(window as unknown as { __chipFresh(): void }).__chipFresh = () => { orgApi.view = view; };
		});
		const remove = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await focusTarget(page, remove);
		await remove.press('Enter');
		await expect(page.getByText('Removed Beacon from Web.', { exact: true })).toBeVisible();
		await expect(remove).toHaveCount(1);
		const next = page.getByRole('button', { name: screen === 'chart' ? 'Actions for Web' : 'Remove Beacon from Design', exact: true });
		await expect(next).toBeFocused();
		await shot(page, `bug115-${screen}-retained-chip`, { context: next });
		await page.evaluate(() => (window as unknown as { __chipFresh(): void }).__chipFresh());
		await page.reload();
		await expect(remove).toHaveCount(0);
	});
}

for (const screen of ['projects', 'profile', 'chart'] as const) {
	test(`TEAM-8 BUG-70 BUG-115: ${screen} cancelled refusal refresh never runs success focus recovery`, async ({ page }) => {
		await create(page, 1280);
		await visit(page, screen === 'profile' ? '/projects/beacon' : screen === 'chart' ? '/org-chart' : '/projects');
		const first = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
		await focusTarget(page, first);
		await first.press('Enter');
		await expect(first).toHaveCount(0);
		const last = page.getByRole('button', { name: 'Remove Beacon from Design', exact: true });
		await last.focus();
		await last.press('Enter');
		const dialog = page.getByRole('dialog', { name: 'Remove Beacon from Design?', exact: true });
		await expect(dialog).toBeVisible();
		await page.evaluate(async () => {
			const modulePath = '/e2e/fakes/org-api.ts';
			const { orgApi } = await import(modulePath) as { orgApi: import('../src/lib/org/api').OrgApi };
			const command = orgApi.command, view = orgApi.view;
			let refused = false, release!: () => void;
			const held = new Promise<void>((resolve) => { release = resolve; });
			(window as unknown as { __chipRefusalRelease(): void }).__chipRefusalRelease = release;
			orgApi.command = async (id, input) => {
				if (input.method === 'project.remove_team') { refused = true; throw { source: 'api', code: 'unavailable', message: 'Refused while refresh is held.' }; }
				return command(id, input);
			};
			orgApi.view = async (id) => {
				if (refused) await held;
				const result = await view(id);
				return refused ? { ...result, snapshot: { ...result.snapshot, teams: result.snapshot.teams.map((t) => t.id === 'design' ? { ...t, name: 'Design refreshed' } : t) } } : result;
			};
		});
		await dialog.getByRole('button', { name: 'Remove', exact: true }).click();
		await expect(dialog.getByRole('alert')).toHaveText('Refused while refresh is held.');
		await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
		await expect(dialog).toBeHidden();
		await expect(last).toBeFocused();
		await page.evaluate(() => (window as unknown as { __chipRefusalRelease(): void }).__chipRefusalRelease());
		const refreshed = page.getByRole('button', { name: 'Remove Beacon from Design refreshed', exact: true });
		await expect(refreshed).toBeVisible();
		await shot(page, `bug115-${screen}-cancelled-refusal`, { context: refreshed });
		await expect(refreshed).toBeFocused();
	});
}

for (const screen of ['projects', 'profile', 'chart'] as const) {
	test(`TEAM-8 BUG-70 BUG-115: ${screen} skips a neighbour that disappeared during the request`, async ({ page }) => {
		await create(page, 1280);
		await chipFixture(page);
		await visit(page, screen === 'profile' ? '/projects/beacon' : screen === 'chart' ? '/org-chart' : '/projects');
		await holdUnlink(page);
		const remove = page.getByRole('button', { name: screen === 'chart' ? 'Remove Atlas from Web' : 'Remove Beacon from Design', exact: true });
		await focusTarget(page, remove);
		await remove.press('Enter');
		await page.waitForFunction(() => (window as unknown as { __chipHold: { entered: boolean } }).__chipHold.entered);
		await page.evaluate(async (screen) => {
			const connections = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]');
			const id = connections[connections.length - 1].organization.id;
			await (window as unknown as { __mockOrgChange(id: string, command: object): Promise<unknown> }).__mockOrgChange(id,
				{ method: 'project.remove_team', params: screen === 'chart'
					? { project_team_id: 'pt-2', expected_version: 1, areas_to_team_id: 'design' }
					: { project_team_id: 'chip-fixture-0', expected_version: 1 } });
		}, screen);
		await expect(page.getByRole('button', { name: screen === 'chart' ? 'Remove Beacon from Web' : 'Remove Beacon from Platform', exact: true })).toHaveCount(0);
		await expect(remove).toBeFocused();
		await releaseUnlink(page);
		await expect(remove).toHaveCount(0);
		const next = page.getByRole('button', { name: screen === 'chart' ? 'Remove Scratchpad from Web' : 'Remove Beacon from Web', exact: true });
		await expect(next).toBeFocused();
		await shot(page, `bug115-${screen}-missing-neighbour`, { context: next });
	});
}

test('TEAM-8 BUG-70 BUG-115: chart confirmation retains its own focus plan across a later cancelled removal', async ({ page }) => {
	await create(page, 1280);
	await visit(page, '/org-chart');
	const first = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
	await focusTarget(page, first);
	await first.press('Enter');
	await expect(first).toHaveCount(0);
	const last = page.getByRole('button', { name: 'Remove Beacon from Design', exact: true });
	await last.focus();
	await last.press('Enter');
	const oldDialog = page.getByRole('dialog', { name: 'Remove Beacon from Design?', exact: true });
	await expect(oldDialog).toBeVisible();
	await page.evaluate(async () => {
		const modulePath = '/e2e/fakes/org-api.ts';
		const { orgApi } = await import(modulePath) as { orgApi: import('../src/lib/org/api').OrgApi };
		const command = orgApi.command, view = orgApi.view;
		let pending = false, release!: () => void;
		const held = new Promise<void>((resolve) => { release = resolve; });
		const hooks = window as unknown as { __chipRefresh: { entered: boolean; release(): void } };
		hooks.__chipRefresh = { entered: false, release };
		orgApi.command = async (id, input) => {
			if (input.method === 'project.remove_team') pending = true;
			return command(id, input);
		};
		orgApi.view = async (id) => {
			if (pending) { hooks.__chipRefresh.entered = true; await held; }
			return view(id);
		};
	});
	await oldDialog.getByRole('button', { name: 'Remove', exact: true }).click();
	await expect(oldDialog).toBeHidden();
	await page.waitForFunction(() => (window as unknown as { __chipRefresh: { entered: boolean } }).__chipRefresh.entered);
	const later = page.getByRole('button', { name: 'Remove Atlas from Platform', exact: true });
	await later.focus();
	await later.press('Enter');
	const nextDialog = page.getByRole('dialog', { name: 'Remove Atlas from Platform?', exact: true });
	await expect(nextDialog).toBeVisible();
	await nextDialog.getByRole('button', { name: 'Cancel', exact: true }).click();
	await expect(nextDialog).toBeHidden();
	await expect(later).toBeFocused();
	await page.evaluate(() => (window as unknown as { __chipRefresh: { release(): void } }).__chipRefresh.release());
	await expect(last).toHaveCount(0);
	await shot(page, 'bug115-chart-confirmation-plan-race', { context: later });
	await expect(later).toBeFocused();
});
