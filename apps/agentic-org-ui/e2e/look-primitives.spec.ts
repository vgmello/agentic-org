import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { enterMockedOrganization } from './mock-organization';

async function open(page: Page, path = '/agents') {
	await enterMockedOrganization(page);
	await expect(page).toHaveURL(/\/teams\/members$/);
	if (path !== '/agents') await visit(page, path);
}

async function tokenColor(page: Page, token: string) {
	return page.evaluate((token) => {
		const reference = document.createElement('span');
		reference.style.color = `var(${token})`;
		document.body.append(reference);
		const color = getComputedStyle(reference).color;
		reference.remove();
		return color;
	}, token);
}

async function css(locator: Locator, property: string) {
	return locator.evaluate(
		(element, property) => getComputedStyle(element).getPropertyValue(property),
		property
	);
}

async function contrast(locator: Locator) {
	return locator.evaluate((element) => {
		// These screens use flat colors. Resolve modern CSS colors to sRGB and composite their real ancestor tints.
		const canvas = document.createElement('canvas');
		canvas.width = canvas.height = 1;
		const context = canvas.getContext('2d')!;
		const rgba = (color: string) => {
			context.clearRect(0, 0, 1, 1);
			context.fillStyle = color;
			context.fillRect(0, 0, 1, 1);
			return [...context.getImageData(0, 0, 1, 1).data];
		};
		const backgrounds = [];
		for (let node: Element | null = element; node; node = node.parentElement)
			backgrounds.push(rgba(getComputedStyle(node).backgroundColor));
		const over = (front: number[], back: number[]) =>
			back.map((channel, i) => (front[i] * front[3]) / 255 + channel * (1 - front[3] / 255));
		const background = backgrounds
			.reverse()
			.reduce((back, front) => over(front, back), [255, 255, 255]);
		const foreground = over(rgba(getComputedStyle(element).color), background);
		const luminance = (color: number[]) =>
			color
				.map((c) => c / 255)
				.map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4))
				.reduce((sum, c, i) => sum + c * [0.2126, 0.7152, 0.0722][i], 0);
		const a = luminance(foreground),
			b = luminance(background);
		return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
	});
}

async function fits(locator: Locator) {
	return locator.evaluate((element) => {
		const box = element.getBoundingClientRect();
		const range = document.createRange();
		range.selectNodeContents(element);
		const rects = [...range.getClientRects()];
		return (
			rects.length > 0 &&
			rects.every((rect) => rect.left >= box.left - 1 && rect.right <= box.right + 1)
		);
	});
}

for (const scale of [1, 2]) {
	test(
		`AC-IA08 F-3 BUG-76: a directory presence pill keeps whole words at 390px and ${scale * 100}% text`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width: 390, height: 900 });
			await open(page, '/teams/members');
			await page.locator('html').evaluate((html, pixels) => {
				html.style.fontSize = pixels + 'px';
			}, 16 * scale);
			const status = page.locator('[data-member=noor] [data-presence]');
			await expect(status).toHaveText('Available');
			await shot(page, `members-presence-390-${scale}`, { context: status });
			const label = status.locator(':scope > span').last();
			const size = await label.evaluate((element) => ({
				height: element.getBoundingClientRect().height,
				line: parseFloat(getComputedStyle(element).lineHeight)
			}));
			expect(size.height).toBeCloseTo(size.line, 0);
			await expect(label).toHaveAttribute('title', 'Available');
			expect(await status.ariaSnapshot()).toContain('Available');
			// A long presence wraps between whole words inside its row (app-shell-ux §6a); it is never clipped.
			const long = page.locator('[data-member=kai] [data-presence]');
			await expect(long).toContainText('Waiting for an answer from ');
			expect(
				await long.evaluate((pill) => {
					const label = pill.lastElementChild as HTMLElement,
						row = pill.closest('li')!.getBoundingClientRect(),
						box = pill.getBoundingClientRect();
					return (
						label.scrollWidth <= label.clientWidth + 1 &&
						box.left >= row.left - 1 &&
						box.right <= row.right + 1
					);
				})
			).toBe(true);
			expect(await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)).toBe(0);
		}
	);
}

for (const width of [320, 640, 960, 1280]) {
	test(
		`AC-IA08 BUG-70: a readable outlined presence pill preserves the directory and its filters at ${width}px`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width, height: 900 });
			await open(page, '/teams/members');
			const list = page.getByRole('list', { name: 'Members', exact: true });
			await expect(list.getByRole('listitem')).toHaveCount(12);
			const noor = list.locator('[data-member=noor]');
			const status = noor.locator('[data-presence]');
			await expect(status).toHaveText('Available');
			const style = await status.evaluate((element) => {
				const s = getComputedStyle(element);
				return {
					radius: parseFloat(s.borderRadius),
					x: parseFloat(s.paddingLeft),
					y: parseFloat(s.paddingTop),
					shadow: s.boxShadow
				};
			});
			expect.soft(style.radius).toBeGreaterThanOrEqual(12);
			expect.soft(style.x).toBe(8);
			expect.soft(style.y).toBe(4);
			expect.soft(style.shadow).not.toBe('none');
			expect.soft(await css(status, 'color')).toBe(await tokenColor(page, '--color-green-300'));
			const ratio = await contrast(status);
			expect.soft(ratio).toBeGreaterThanOrEqual(4.5);
			console.info(`Presence text contrast at ${width}px: ${ratio.toFixed(2)}:1`);
			await expect(status.locator('svg')).toHaveAttribute('aria-hidden', 'true');
			await expect(
				noor.getByRole('link', { name: 'Noor Haddad, agent', exact: true })
			).toHaveAttribute('href', '/members/noor');
			const agents = page
				.getByRole('group', { name: 'Show members', exact: true })
				.getByRole('radio', { name: /^Agents / });
			await agents.focus();
			await agents.press('Space');
			await expect(agents).toBeFocused();
			await expect(agents).toBeChecked();
			await expect(list.getByRole('listitem')).toHaveCount(8);
			await status.scrollIntoViewIfNeeded();
			await shot(page, `look-pill-${width}`, { context: status });
		}
	);
	test(
		`PROJ-1 BUG-70: native divided rows and a quiet neutral callout at ${width}px`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width, height: 900 });
			await open(page, '/projects');
			const list = page.getByRole('main').getByRole('list', { name: 'Projects', exact: true });
			await expect(list.getByRole('listitem')).toHaveCount(4);
			await expect(list.getByTestId('project-row').getByRole('link')).toHaveText([
				'Atlas',
				'Beacon',
				'Onboarding kit',
				'Scratchpad'
			]);
			const first = list.getByRole('listitem').first();
			expect.soft(await css(first, 'padding-top')).toBe('16px');
			expect.soft(await css(first, 'padding-bottom')).toBe('16px');
			expect.soft(await css(list, 'border-radius')).toBe('0px');
			expect.soft(await css(list, 'background-color')).toBe('rgba(0, 0, 0, 0)');
			const note = page.getByTestId('unassigned-note');
			await expect(note).toHaveText('1 project has no team yet.');
			expect.soft(await css(note, 'color')).toBe(await tokenColor(page, '--color-gray-300'));
			expect(await contrast(note)).toBeGreaterThanOrEqual(4.5);
			expect.soft(await note.locator('svg[aria-hidden=true]').count()).toBe(1);
			expect(await note.getAttribute('role')).toBeNull(); // Static context must not acquire Alert's default live role.
			expect(await note.getAttribute('aria-live')).toBeNull();
			expect(await fits(note)).toBe(true);
			await expect(page.getByRole('button', { name: 'New project', exact: true })).toBeDisabled();
			await shot(page, `look-projects-${width}`, { context: list });
		}
	);
}

for (const width of [320, 960]) {
	test(
		`AC-IA08 BUG-70 BUG-76: a directory presence pill wraps whole words inside a constrained row at 200% text and ${width}px`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width, height: 900 });
			await open(page, '/teams/members');
			await page.addStyleTag({ content: 'html { font-size: 200%; }' });
			const list = page.getByTestId('members-list');
			// Stress the real row in a 10rem list: StatusPill truncates elsewhere, the directory wraps it (app-shell-ux §6a).
			await list.evaluate((element) => {
				element.style.width = '10rem';
			});
			const status = list.locator('[data-member=kai] [data-presence]');
			await expect(status).toContainText('Waiting for an answer from Mira Castillo');
			const label = status.locator(':scope > span').last();
			await expect(label).toHaveCSS('white-space', 'normal');
			await expect(label).toHaveCSS('overflow-x', 'visible');
			expect(
				await label.evaluate((element) => element.scrollWidth <= element.clientWidth + 1)
			).toBe(true);
			// Whole words: no line ends inside a word, so every line starts where a word starts.
			expect(
				await label.evaluate((element) => {
					const text = element.textContent!.trim();
					const range = document.createRange();
					const node = [...element.childNodes].find((n) => n.nodeType === Node.TEXT_NODE)!;
					const tops = new Set<number>();
					let breaks = 0;
					for (let i = 0; i < node.textContent!.length; i++) {
						range.setStart(node, i);
						range.setEnd(node, i + 1);
						const top = Math.round(range.getBoundingClientRect().top);
						if (tops.size && !tops.has(top) && i > 0 && node.textContent![i - 1] !== ' ') breaks++;
						tops.add(top);
					}
					return { lines: tops.size, breaks, text };
				})
			).toMatchObject({ breaks: 0 });
			expect(await status.ariaSnapshot()).toContain('Waiting for an answer from Mira Castillo');
			expect(
				await status.evaluate(
					(element) =>
						element.getBoundingClientRect().right <=
						element.closest('li')!.getBoundingClientRect().right + 1
				)
			).toBe(true);
			await status.scrollIntoViewIfNeeded();
			await shot(page, `look-pill-200-${width}`, { context: status });
		}
	);

	test(
		`PROJ-1 BUG-70: a long project name and callout wrap at 200% text and ${width}px`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width, height: 900 });
			await open(page);
			const long = 'Project' + 'x'.repeat(64);
			await page.evaluate(async (name) => {
				const path = '/e2e/fakes/org-api.ts';
				const { orgApi } = await import(path);
				const view = orgApi.view.bind(orgApi);
				orgApi.view = async (id: string) => {
					const result = await view(id);
					return {
						...result,
						snapshot: {
							...result.snapshot,
							projects: result.snapshot.projects.map((p: { id: string }) =>
								p.id === 'scratch' ? { ...p, name } : p
							)
						}
					};
				};
			}, long);
			if (width < 640) await page.getByRole('button', { name: 'Menu', exact: true }).click();
			await page.getByRole('link', { name: 'Projects', exact: true }).first().click();
			await expect(page).toHaveURL(/\/projects$/);
			await page.addStyleTag({ content: 'html { font-size: 200%; }' });
			const row = page.locator('[data-testid=project-row][data-project=scratch]');
			const link = row.getByRole('link', { name: long, exact: true });
			await expect(link).toBeVisible();
			expect(await fits(link)).toBe(true);
			expect(await fits(page.getByTestId('unassigned-note'))).toBe(true);
			// A compact icon column must yield before ordinary words are split into letters.
			const wordLines = await page.getByTestId('unassigned-note').evaluate((element) => {
				const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
				for (let node = walker.nextNode(); node; node = walker.nextNode()) {
					const start = node.textContent!.indexOf('project');
					if (start < 0) continue;
					const range = document.createRange();
					range.setStart(node, start);
					range.setEnd(node, start + 'project'.length);
					return range.getClientRects().length;
				}
				throw new Error('The project word is missing');
			});
			expect(wordLines).toBe(1);
			expect(await css(row, 'padding-top')).toBe('32px');
			await shot(page, `look-callout-200-${width}`, {
				context: page.getByTestId('unassigned-note')
			});
			await shot(page, `look-projects-long-200-${width}`, { context: row });
		}
	);
}

test(
	'AC-IA08 BUG-70: Paused keeps its word, neutral tint and keyboard filter in the directory',
	{ tag: '@webkit' },
	async ({ page }) => {
		await open(page);
		// Pause from the agent's profile (More › Pause…).
		await visit(page, '/members/noor');
		await page.getByRole('button', { name: /^More/ }).click();
		await page.getByRole('menuitem', { name: 'Pause…', exact: true }).click();
		await page.getByRole('dialog').getByRole('button', { name: 'Pause', exact: true }).click();
		await expect(page.getByText('Pause requested for Noor Haddad.')).toBeVisible();
		await visit(page, '/teams/members');
		const agents = page
			.getByRole('group', { name: 'Show members', exact: true })
			.getByRole('radio', { name: /^Agents / });
		await agents.focus();
		await agents.press('Space');
		await expect(agents).toBeFocused();
		await expect(
			page.getByRole('list', { name: 'Members', exact: true }).getByRole('listitem')
		).toHaveCount(8);
		const status = page.locator('[data-member=noor] [data-presence]');
		await expect(status).toHaveText('Paused');
		// Amber means waiting on the owner (§6a); Paused is a neutral state.
		expect(await css(status, 'color')).toBe(await tokenColor(page, '--color-gray-300'));
		expect(await contrast(status)).toBeGreaterThanOrEqual(4.5);
		expect(await css(status, 'padding-top')).toBe('4px');
		await shot(page, 'look-pill-paused', { context: status });
	}
);

test(
	'PROJ-2/3 BUG-70: row links and keyboard assignment keep their behavior',
	{ tag: '@webkit' },
	async ({ page }) => {
		await open(page, '/projects');
		const row = page.locator('[data-testid=project-row][data-project=scratch]');
		const assign = row.getByRole('button', { name: 'Assign a team to Scratchpad', exact: true });
		await assign.focus();
		await assign.press('Enter');
		const dialog = page.getByRole('dialog', { name: 'Assign a team to Scratchpad', exact: true });
		await dialog.getByRole('radio', { name: 'Design', exact: true }).check();
		await dialog.getByRole('button', { name: 'Assign team', exact: true }).click();
		await expect(page.getByText('Scratchpad now belongs to Design.')).toBeVisible();
		await expect(page.getByTestId('unassigned-note')).toHaveCount(0);
		await expect(row.getByTestId('project-team')).toHaveText(/Design/);
		await expect(assign).toBeFocused();
		const link = row.getByRole('link', { name: 'Scratchpad', exact: true });
		await link.focus();
		await link.press('Enter');
		await expect(page).toHaveURL(/\/projects\/scratch$/);
		await expect(page.getByTestId('project-name')).toHaveText('Scratchpad');
	}
);

for (const width of [320, 640, 960, 1280])
	for (const scale of [1, 2]) {
		test(
			`F-3 BUG-70 BUG-76: primitives keep status words, caller roles and native list attributes at ${width}px and ${scale * 100}% text`,
			{ tag: '@webkit' },
			async ({ page }) => {
				await page.setViewportSize({ width, height: 1000 });
				await open(page);
				await page.locator('html').evaluate((html, pixels) => {
					html.style.fontSize = pixels + 'px';
				}, 16 * scale);
				await page.evaluate(async () => {
					const path = '/src/lib/components/testing/look-primitives.ts';
					const { mountPrimitives } = await import(path);
					const target = document.createElement('div');
					document.body.append(target);
					mountPrimitives(target);
				});
				const fixture = page.getByRole('region', { name: 'Primitive contracts', exact: true });
				await expect(fixture).toBeVisible();
				await page.evaluate(() => document.fonts.ready.then(() => undefined));
				await expect(fixture.getByTestId('word-only')).toHaveText('Status unknown');
				await expect(fixture.getByTestId('word-only').locator('svg')).toHaveCount(0);
				await expect(fixture.getByTestId('word-and-icon')).toHaveText('Paused');
				await expect(fixture.getByTestId('word-and-icon').locator('svg')).toBeVisible();
				await expect(
					fixture.getByRole('img', { name: 'Status ornament', exact: true })
				).toHaveCount(0);
				await expect(fixture.getByTestId('icon-callout').locator('svg')).toBeVisible();
				await expect(
					fixture.getByRole('img', { name: 'Callout ornament', exact: true })
				).toHaveCount(0);
				const longLabel = fixture.getByTestId('long-label');
				await expect(longLabel).toHaveText('Status' + 'x'.repeat(64));
				const token = 'Status' + 'x'.repeat(64);
				const label = longLabel.locator(':scope > span').last();
				await expect(label).toHaveAttribute('title', token);
				await expect(label).toHaveCSS('white-space', 'nowrap');
				await expect(label).toHaveCSS('text-overflow', 'ellipsis');
				await expect(label).toHaveCSS('overflow-x', 'hidden');
				expect(await label.evaluate((element) => element.scrollWidth > element.clientWidth)).toBe(
					true
				);
				expect(await longLabel.ariaSnapshot()).toContain(token);
				const icon = longLabel.locator('svg');
				await expect(icon).toBeVisible();
				expect(
					await icon.evaluate((element) => {
						const box = element.getBoundingClientRect(),
							pill = element.closest('[data-slot=badge]')!.getBoundingClientRect();
						return (
							box.width > 0 &&
							box.left >= pill.left &&
							box.right <= pill.right &&
							box.top >= pill.top &&
							box.bottom <= pill.bottom
						);
					})
				).toBe(true);
				const words = fixture.getByTestId('word-only').locator(':scope > span').last();
				await expect(words).toHaveCSS('white-space', 'nowrap');
				expect(
					await words.evaluate((element) => {
						const range = document.createRange();
						range.selectNodeContents(element);
						return new Set([...range.getClientRects()].map((r) => Math.round(r.top))).size;
					})
				).toBe(1);
				expect(
					await longLabel.evaluate(
						(element) => element.getBoundingClientRect().width <= element.parentElement!.clientWidth
					)
				).toBe(true);
				expect(await fits(fixture.getByTestId('long-callout'))).toBe(true);
				const wrapping = fixture.getByTestId('wrapping-pills');
				const pills = wrapping.locator('[data-slot=badge]');
				await expect(pills).toHaveText(['On hold', 'Ready now']);
				// The row wraps between intact pills; neither label is squeezed into multiple lines.
				const boxes = await pills.evaluateAll((elements) =>
					elements.map((pill) => {
						const label = pill.lastElementChild!,
							css = getComputedStyle(label);
						const range = document.createRange();
						range.selectNodeContents(label);
						return {
							top: pill.getBoundingClientRect().top,
							height: label.getBoundingClientRect().height,
							line: parseFloat(css.lineHeight),
							lines: new Set([...range.getClientRects()].map((r) => Math.round(r.top))).size,
							clipped: label.scrollWidth > label.clientWidth
						};
					})
				);
				expect(boxes[1].top).toBeGreaterThan(boxes[0].top);
				for (const box of boxes) {
					expect(box.height).toBeCloseTo(box.line, 0);
					expect(box.lines).toBe(1);
					expect(box.clipped).toBe(false);
				}
				const quiet = fixture.getByTestId('quiet-contract');
				expect(await quiet.getAttribute('role')).toBeNull();
				expect(await quiet.getAttribute('aria-live')).toBeNull();
				await expect(fixture.getByRole('status')).toHaveText('Saved progress');
				await expect(fixture.getByRole('alert')).toHaveText('Failed update');
				await expect(fixture.getByRole('status')).toHaveCount(1);
				await expect(fixture.getByRole('alert')).toHaveCount(1);
				const row = fixture
					.getByRole('list', { name: 'Ordered facts', exact: true })
					.getByRole('listitem');
				await expect(row).toHaveAttribute('value', '7');
				await expect(row).toHaveAttribute('lang', 'pt');
				await expect(row).toHaveAttribute('title', 'Native row');
				await expect(row).toHaveAttribute('data-testid', 'native-row');
				expect(await row.getAttribute('role')).toBeNull();
				expect(await row.getAttribute('tabindex')).toBeNull();
				expect(await row.getAttribute('onclick')).toBeNull();
				await shot(page, `look-primitives-contracts-${width}-${scale}`, { context: fixture });
			}
		);
	}

for (const width of [320, 640, 960, 1280])
	for (const scale of [1, 2]) {
		test(
			'PEOPLE-1 BUG-70: the flat directory keeps all states at ' +
				width +
				'px and ' +
				scale * 100 +
				'% text',
			{ tag: '@webkit' },
			async ({ page }) => {
				await open(page);
				expect((await page.request.get('/e2e/fakes/org-api.ts')).status()).toBe(200);
				// Existing user flows provide real Paused and Former rows in the test-owned org.
				// Pause and Offboard from each member's profile (More menu).
				await visit(page, '/members/noor');
				await page.getByRole('button', { name: /^More/ }).click();
				await page.getByRole('menuitem', { name: 'Pause…', exact: true }).click();
				await page.getByRole('dialog').getByRole('button', { name: 'Pause', exact: true }).click();
				await expect(page.getByText('Pause requested for Noor Haddad.')).toBeVisible();
				await visit(page, '/members/sam');
				await page.getByRole('button', { name: /^More/ }).click();
				await page.getByRole('menuitem', { name: 'Offboard…', exact: true }).click();
				await page
					.getByRole('dialog')
					.getByRole('button', { name: 'Offboard', exact: true })
					.click();
				await expect(page.getByText('Offboarded Sam Okafor.')).toBeVisible();
				await visit(page, '/teams/members');
				await page.setViewportSize({ width, height: 900 });
				await page.locator('html').evaluate((html, pixels) => {
					html.style.fontSize = pixels + 'px';
				}, 16 * scale);
				const filters = page.getByRole('group', { name: 'Show members', exact: true });
				const list = page.getByRole('list', { name: 'Members', exact: true });
				await expect(filters.getByRole('radio')).toHaveCount(3);
				for (const [name, count] of [
					['All', 11],
					['People', 4],
					['Agents', 7]
				] as const) {
					const radio = filters.getByRole('radio', { name: new RegExp('^' + name + ' ') });
					await radio.focus();
					await radio.press('Space');
					await expect(radio).toBeFocused();
					await expect(radio).toBeChecked();
					await expect(list.getByRole('listitem')).toHaveCount(count);
					// Flat divided rows: no tinted cards, no table, no horizontal scroller (§6a).
					await expect(page.getByRole('table')).toHaveCount(0);
					expect.soft(await css(list, 'background-color')).toBe('rgba(0, 0, 0, 0)');
					expect.soft(await css(list, 'border-radius')).toBe('0px');
					const first = list.getByRole('listitem').first();
					expect.soft(parseFloat(await css(first, 'padding-top'))).toBe(16 * scale);
					expect.soft(parseFloat(await css(first, 'padding-bottom'))).toBe(16 * scale);
					// Tailwind 4 divides with a bottom border on every row but the last.
					if (count > 1) expect.soft(await css(first, 'border-bottom-width')).toBe('1px');
					if (name !== 'People') {
						const paused = list.locator('[data-member=noor] [data-presence]');
						await expect(paused).toHaveText('Paused');
						await expect
							.poll(async () => parseFloat(await css(paused, 'padding-top')))
							.toBe(4 * scale);
					}
					// The first row's words stay inside the row.
					const readable = await first.evaluate((row) => {
						const box = row.getBoundingClientRect();
						const text = document.createTreeWalker(row, NodeFilter.SHOW_TEXT);
						const glyphs: DOMRect[] = [];
						while (text.nextNode()) {
							const parent = text.currentNode.parentElement!;
							if (!text.currentNode.textContent?.trim() || parent.closest('.sr-only')) continue;
							const range = document.createRange();
							range.selectNodeContents(text.currentNode);
							glyphs.push(...range.getClientRects());
						}
						return {
							count: glyphs.length,
							fits: glyphs.every(
								(r) =>
									r.left >= Math.max(0, box.left) - 1 &&
									r.right <= Math.min(innerWidth, box.right) + 1
							)
						};
					});
					expect.soft(readable.count).toBeGreaterThan(0);
					expect.soft(readable.fits).toBe(true);
					expect(await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)).toBe(
						0
					);
					await shot(page, 'members-flat-' + width + '-' + scale + '-' + name);
				}
				await filters.getByRole('radio', { name: /^All / }).press('Space');
				const former = page.getByTestId('former-members');
				await former.getByText('Former members').click();
				const sam = page
					.getByRole('list', { name: 'Former members', exact: true })
					.locator('[data-member=sam]');
				await expect(sam).toContainText(/Former member|Left /);
				await expect(sam.locator('[data-presence]')).toHaveCount(0);
				const link = list
					.locator('[data-member=noor]')
					.getByRole('link', { name: 'Noor Haddad, agent', exact: true });
				await expect(link).toHaveAttribute('href', '/members/noor');
				await link.focus();
				await link.press('Enter');
				await expect(page).toHaveURL(/\/members\/noor$/);
				await page.goBack();
				await expect(list.getByRole('listitem')).toHaveCount(11);
				await visit(page, '/teams/members?mock=empty');
				await page.locator('html').evaluate((html, pixels) => {
					html.style.fontSize = pixels + 'px';
				}, 16 * scale);
				await expect(
					page.getByRole('heading', { name: 'No members yet', exact: true })
				).toBeVisible();
				await expect(list).toHaveCount(0);
				await shot(page, 'members-flat-' + width + '-' + scale + '-empty');
			}
		);
	}
