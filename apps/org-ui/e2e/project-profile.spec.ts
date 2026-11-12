import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

const overview = 'Features, bugs, debt, documents and insights for this project are available in a later release.';
async function create(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	expect((await page.request.get('/e2e/fakes/org-api.ts')).status()).toBe(200);
}
async function open(page: Page, path: string, scale = 1) {
	await visit(page, path);
	if (scale === 2) await page.addStyleTag({ content: 'html { font-size: 200%; }' });
}
const header = (page: Page) => page.locator('header').filter({ has: page.getByTestId('project-name') });
async function css(locator: Locator, property: string) {
	return locator.evaluate((element, property) => getComputedStyle(element).getPropertyValue(property), property);
}
async function inViewport(locator: Locator) {
	return locator.evaluate((element) => {
		const range = document.createRange();
		range.selectNodeContents(element);
		const rects = [...range.getClientRects()];
		return rects.length > 0 && rects.every((r) => r.left >= -1 && r.right <= innerWidth + 1);
	});
}

for (const width of [320, 640, 960, 1280]) for (const scale of [1, 2]) {
	test(`PROJ-3 BUG-70: flat readable Project profile at ${width}px and ${scale * 100}% text`, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await create(page);
		await open(page, '/projects/beacon', scale);
		const title = page.getByTestId('project-name');
		const breadcrumb = page.getByRole('navigation', { name: 'Breadcrumb', exact: true });
		await expect(title).toHaveText('Beacon');
		await expect(header(page).getByText('Sample data', { exact: true })).toHaveCount(1);
		await expect(page.getByTestId('leads-line')).toHaveText('Leads: Juno Park (Design), Noor Haddad (Web)');
		const font = await page.locator('html').evaluate((element) => parseFloat(getComputedStyle(element).fontSize));
		const mainInset = await page.getByRole('main').evaluate((element) => parseFloat(getComputedStyle(element).paddingLeft));
		const expectedLeft = mainInset + font * (width >= 1024 ? 2 : width >= 640 ? 1.5 : 1);
		const crumbBox = (await breadcrumb.boundingBox())!;
		expect.soft(crumbBox.x).toBeCloseTo(expectedLeft, 0);
		expect.soft(await css(header(page), 'background-color')).toBe('rgba(0, 0, 0, 0)');
		expect.soft(await css(header(page), 'border-radius')).toBe('0px');
		expect.soft(parseFloat(await css(title, 'font-size'))).toBe(font);
		expect.soft((await title.boundingBox())!.x).toBeCloseTo(crumbBox.x, 0);
		expect.soft((await page.getByTestId('project-team').first().boundingBox())!.x).toBeCloseTo(crumbBox.x, 0);
		expect.soft(await page.getByRole('list', { name: 'Project details', exact: true }).count()).toBe(1);
		const rowPadding = await page.getByTestId('leads-line').evaluate((element) => {
			const row = element.closest('li');
			return row ? parseFloat(getComputedStyle(row).paddingTop) : 0;
		});
		expect.soft(rowPadding).toBe(font);
		const note = page.getByText(overview, { exact: true });
		await expect(note).toBeVisible();
		expect(await note.evaluate((element) => element.closest('[role=alert], [role=status], [aria-live]') !== null)).toBe(false);
		await shot(page, `project-profile-${width}-${scale}`, { context: header(page) });

		await open(page, '/projects/ghost', scale);
		const missing = page.getByTestId('project-gone');
		await expect(missing).toContainText("This project isn't here any more.");
		expect.soft(await css(missing, 'text-align')).toBe('left');
		expect(await missing.getAttribute('role')).toBeNull();
		expect(await missing.getAttribute('aria-live')).toBeNull();
		const back = missing.getByRole('link', { name: 'Go to Projects', exact: true });
		await expect(back).toHaveAttribute('href', '/projects');
		await expect(back).not.toBeFocused();
		expect.soft(await css(back, 'outline-style')).toBe('none');
		await back.focus();
		await expect(back).toBeFocused();
		expect.soft(await css(back, 'outline-style')).toBe('solid');
		expect.soft(parseFloat(await css(back, 'outline-width'))).toBeGreaterThanOrEqual(2);
		expect.soft(await inViewport(back)).toBe(true);
		await shot(page, `project-profile-${width}-${scale}-missing`, { context: missing });
		await back.press('Enter');
		await expect(page).toHaveURL(/\/projects$/);
		await expect(page.getByTestId('project-row')).toHaveCount(4);
	});
}

test('PROJ-3 BUG-70: gutter breakpoints hold below, at and above each boundary', async ({ page }) => {
	await create(page);
	await open(page, '/projects/beacon');
	for (const width of [639, 640, 641, 1023, 1024, 1025]) {
		await page.setViewportSize({ width, height: 900 });
		const font = await page.locator('html').evaluate((element) => parseFloat(getComputedStyle(element).fontSize));
		const expected = font * (width >= 1024 ? 2 : width >= 640 ? 1.5 : 1);
		await expect.poll(async () => {
			const left = (await page.getByRole('navigation', { name: 'Breadcrumb', exact: true }).boundingBox())!.x;
			const inset = await page.getByRole('main').evaluate((element) => parseFloat(getComputedStyle(element).paddingLeft));
			return left - inset;
		}).toBeCloseTo(expected, 0);
		expect(await inViewport(page.getByTestId('project-name'))).toBe(true);
	}
	await shot(page, 'project-profile-breakpoints');
});

for (const [width, scale] of [[320, 2], [1280, 1]] as const) {
	test(`PROJ-3 BUG-70: long Project name without a sample badge at ${width}px`, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await create(page);
		const name = 'Beacon' + 'W'.repeat(64);
		await page.evaluate(async (name) => {
			const path = '/e2e/fakes/org-api.ts';
			const { orgApi } = await import(path);
			const view = orgApi.view.bind(orgApi);
			orgApi.view = async (id: string) => {
				const result = await view(id);
				return { ...result, sample: false, snapshot: { ...result.snapshot, projects: result.snapshot.projects.map((project: { id: string }) => project.id === 'beacon' ? { ...project, name } : project) } };
			};
		}, name);
		if (width < 640) await page.getByRole('button', { name: 'Menu', exact: true }).click();
		await page.getByRole('link', { name: 'Projects', exact: true }).first().click();
		await page.getByRole('link', { name, exact: true }).click();
		await expect(page).toHaveURL(/\/projects\/beacon$/);
		if (scale === 2) await page.addStyleTag({ content: 'html { font-size: 200%; }' });
		await expect(page.getByTestId('project-name')).toHaveText(name);
		await expect(header(page).getByText('Sample data', { exact: true })).toHaveCount(0);
		expect.soft(await inViewport(page.getByTestId('project-name'))).toBe(true);
		expect.soft(await inViewport(page.getByRole('navigation', { name: 'Breadcrumb', exact: true }))).toBe(true);
		await shot(page, `project-profile-long-${width}`, { context: header(page) });
	});
}

test('PROJ-2/3 BUG-70: Project membership refresh and keyboard tabs keep actions and focus', async ({ page }) => {
	await create(page);
	await open(page, '/projects/scratch');
	await expect(page.getByTestId('no-team')).toHaveText('No team');
	await expect(page.getByTestId('leads-line')).toHaveText('');
	const assign = page.getByRole('button', { name: 'Assign a team to Scratchpad', exact: true });
	await assign.focus();
	await assign.press('Enter');
	const dialog = page.getByRole('dialog', { name: 'Assign a team to Scratchpad', exact: true });
	await dialog.getByRole('radio', { name: 'Design', exact: true }).check();
	await dialog.getByRole('button', { name: 'Assign team', exact: true }).click();
	await expect(page.getByText('Scratchpad now belongs to Design.')).toBeVisible();
	await expect(assign).toBeFocused();
	await expect(page.getByTestId('project-name')).toHaveText('Scratchpad');
	await expect(page.getByTestId('leads-line')).toHaveText('Lead: Juno Park');
	for (const name of ['Tasks', 'Bugs', 'Overview']) {
		const tab = page.getByRole('tab', { name, exact: true });
		await tab.focus();
		await tab.press('Enter');
		await expect(tab).toBeFocused();
		await expect(tab).toHaveAttribute('aria-selected', 'true');
		if (name === 'Tasks') await expect(page.getByTestId('task-board').or(page.getByTestId('task-list'))).toBeVisible();
		if (name === 'Bugs') await expect(page.getByTestId('bug-list').or(page.getByTestId('no-bugs')).or(page.getByTestId('no-match'))).toBeVisible();
	}
	for (const name of ['Features', 'Debt', 'Docs', 'Insights', 'Settings']) {
		await expect(page.getByRole('tab', { name, exact: true })).toHaveAttribute('aria-disabled', 'true');
		await expect(page.getByRole('tab', { name, exact: true })).toHaveAttribute('title', 'Available in a later release');
	}
	await expect(page.getByText(overview, { exact: true })).toBeVisible();
	await shot(page, 'project-profile-actions');
});

const BUG158_PROJECT = '/projects/atlas';
for (const width of [320, 640, 960, 1280]) for (const scale of [1, 2]) {
	test(`BUG-158 BUG-70: TasksBoard outside card acts once and owns the layer at ${width}/${scale * 100}`, async ({ page }) => {
		await page.setViewportSize({ width: 1280, height: 1100 }); await create(page); await open(page, BUG158_PROJECT);
		await page.getByRole('tab', { name: 'Tasks', exact: true }).click();
		await page.getByRole('button', { name: 'New task', exact: true }).click();
		const dialog = page.getByRole('dialog', { name: 'New task', exact: true });
		await dialog.getByLabel('Title', { exact: true }).fill('Pointer menu proof');
		await dialog.getByRole('button', { name: 'Create task', exact: true }).click(); await expect(dialog).toHaveCount(0);
		await page.setViewportSize({ width, height: 1100 });
		await page.locator('html').evaluate((html, scale) => html.style.fontSize = `${scale * 100}%`, scale);
		const card = page.getByTestId('task-card').filter({ hasText: 'Pointer menu proof' });
		const trigger = card.getByRole('button', { name: /^Actions for/ });
		await page.evaluate(()=>document.fonts.ready); await shot(page, `bug158-task-before-open-${width}-${scale}`);
		await trigger.click(); await expect(page.getByRole('menu')).toBeVisible();
		const target = card.getByRole('button').filter({ hasText: 'Pointer menu proof' });
		await target.scrollIntoViewIfNeeded();
		await target.evaluate((node) => { node.dataset.clicks = '0'; node.addEventListener('click', () => node.dataset.clicks = String(Number(node.dataset.clicks) + 1)); });
		await shot(page, `bug158-task-before-outside-${width}-${scale}`); await clickExposedMenuDestination(page, target);
		await expect(target).toHaveAttribute('data-clicks', '1'); await expect(page.getByRole('menu')).toHaveCount(0);
		const destination = page.getByRole('dialog'); await expect(destination).toContainText('Pointer menu proof');
		await expect.poll(() => destination.evaluate((node) => node.contains(document.activeElement))).toBe(true);
		await shot(page, `bug158-task-card-destination-${width}-${scale}`);
	});
}

// A native click must land on an exposed part of the actual control, not on its portal.
async function clickExposedMenuDestination(page: import('@playwright/test').Page, target: import('@playwright/test').Locator) {
 await target.evaluate(node=>node.scrollIntoView({block:'center',inline:'nearest',behavior:'instant'}));
 await page.evaluate(()=>new Promise<void>(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve()))));
 const point = await target.evaluate(node => {
  const r = node.getBoundingClientRect();
  for (const fy of [.5, .1, .9]) for (const fx of [.5, .1, .9]) {
   const x = r.left + r.width * fx, y = r.top + r.height * fy;
   if (x > 0 && x < innerWidth && y > 0 && y < innerHeight && node.contains(document.elementFromPoint(x, y))) return { x, y };
  }
  return null;
 });
 expect(point, 'the outside destination has a real exposed hit point').not.toBeNull();
 await page.mouse.click(point!.x, point!.y);
}
