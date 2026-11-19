import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { enterMockedOrganization } from './mock-organization';

// The team dialogs open from Teams (/teams) and the member dialogs from the member's profile; the dialogs that only the
// retired org-chart canvas opened (team members, add project, change manager, member teams, how it runs, remove
// person) have no caller left.
export const titles = {
	'delete-refused': "Can't delete Engineering",
	task: 'New task',
	report: 'Report a bug',
	assign: 'Assign a team to Scratchpad',
	delete: 'Delete Spare?',
	'edit-person': 'Edit Hana Weiss',
	'move-member': "Move Sam Okafor's home team",
	'move-team': 'Move Engineering to…',
	'new-request': 'New request',
	'remove-project': 'Remove Atlas from Platform?',
	'rename-team': 'Rename Web',
	'block-task': 'Block T-1',
	'edit-agent': 'Edit Sam Okafor',
	offboard: 'Offboard Sam Okafor?',
	pause: 'Pause Sam Okafor?',
	channel: 'Add a channel',
	'new-message': 'New message',
	command: 'Post the project status'
} as const;
export type TitleCase = keyof typeof titles;
// These seven v1 callers had no wrapping override; the others set anywhere.
const originallyNormal = new Set<TitleCase>([
	'task',
	'report',
	'new-request',
	'block-task',
	'channel',
	'new-message',
	'command'
]);
const originalWrap = (kind: TitleCase): 'normal' | 'anywhere' =>
	originallyNormal.has(kind) ? 'normal' : 'anywhere';

/** A top-level team with nothing in it, which Delete accepts. */
export async function createSpareTeam(page: Page) {
	await page.evaluate(async () => {
		const saved = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]');
		const id = saved[saved.length - 1].organization.id as string;
		await (
			window as unknown as { __mockOrgChange: (id: string, command: object) => Promise<unknown> }
		).__mockOrgChange(id, {
			method: 'team.create',
			params: { team_id: 'title-spare', name: 'Spare' }
		});
	});
}

export async function createDialogOrganization(page: Page, width: number) {
	await page.setViewportSize({ width, height: 900 });
	await enterMockedOrganization(page);
	await expect(page).toHaveURL(/\/teams\/members$/);
	await expect(page.getByRole('list', { name: 'Members', exact: true })).toBeVisible();
}

export async function openTitle(page: Page, kind: TitleCase, width: number, scale = 1, root = 16) {
	await createDialogOrganization(page, width);
	if (kind === 'task' || kind === 'report') {
		await visit(page, '/projects/atlas/work');
		await page.getByRole('tab', { name: kind === 'task' ? 'Tasks' : 'Bugs', exact: true }).click();
		await page
			.getByRole('button', { name: kind === 'task' ? 'New task' : 'Report a bug', exact: true })
			.click();
	} else if (kind === 'assign') {
		await visit(page, '/projects');
		await page.getByRole('button', { name: 'Assign a team to Scratchpad', exact: true }).click();
	} else if (kind === 'delete' || kind === 'delete-refused') {
		// Every sample team has members or work (offices-and-teams-ux §7.10), so Delete gets an empty team of its own.
		if (kind === 'delete') await createSpareTeam(page);
		await visit(page, '/teams');
		await page
			.getByRole('button', {
				name: kind === 'delete' ? 'Actions for Spare' : 'Actions for Engineering',
				exact: true
			})
			.click();
		await page.getByRole('menuitem', { name: 'Delete…', exact: true }).click();
	} else {
		if (['channel', 'new-message', 'command'].includes(kind)) {
			await visit(page, '/chat?c=c-general');
			if (kind === 'command') {
				await page.getByTestId('composer').fill('/status atlas');
				await page.getByTestId('composer').press('Control+Enter');
			} else {
				const nav = page.getByRole('navigation', { name: 'Chat', exact: true });
				if (!(await nav.isVisible()))
					await page.getByRole('button', { name: /^Conversations(?:,|$)/ }).click();
				await expect(nav).toBeVisible();
				await nav
					.getByRole('button', {
						name: kind === 'channel' ? 'Add channel' : 'New message',
						exact: true
					})
					.click();
			}
		} else if (kind === 'new-request') {
			await visit(page, '/work');
			await page.getByRole('tab', { name: /^Requests/ }).click();
			await page.getByTestId('new-request').click();
		} else if (kind === 'remove-project') {
			await visit(page, '/projects');
			await page.getByRole('button', { name: 'Remove Atlas from Platform', exact: true }).click();
		} else if (kind === 'block-task') {
			await visit(page, '/projects/atlas/work');
			await page.getByRole('tab', { name: 'Tasks', exact: true }).click();
			await page.getByRole('button', { name: 'New task', exact: true }).click();
			await page.getByRole('dialog').getByLabel('Title', { exact: true }).fill('Layout regression');
			await page
				.getByRole('dialog')
				.getByRole('button', { name: 'Create task', exact: true })
				.click();
			await expect(page.getByRole('dialog')).toHaveCount(0);
			await page
				.getByTestId('task-card')
				.getByRole('button', { name: /^Actions for/ })
				.click();
			await page.getByRole('menuitem', { name: 'Blocked', exact: true }).click();
		} else if (kind === 'move-team' || kind === 'rename-team') {
			await visit(page, '/teams');
			await page
				.getByRole('button', {
					name: `Actions for ${kind === 'move-team' ? 'Engineering' : 'Web'}`,
					exact: true
				})
				.click();
			await page
				.getByRole('menuitem', { name: kind === 'move-team' ? 'Move to…' : 'Rename…', exact: true })
				.click();
		} else {
			const agent = kind !== 'edit-person';
			await visit(page, agent ? '/members/sam' : '/members/hana');
			await page.getByRole('button', { name: /^More/ }).click();
			const name = (
				{
					'edit-person': 'Edit…',
					'edit-agent': 'Edit…',
					'move-member': 'Move home team…',
					offboard: 'Offboard…',
					pause: 'Pause…'
				} as Partial<Record<TitleCase, string>>
			)[kind];
			await page.getByRole('menuitem', { name: name!, exact: true }).click();
		}
	}
	const dialog = page.locator('[data-slot=dialog-content]');
	await expect(dialog).toBeVisible();
	await page
		.locator('html')
		.evaluate((html, scale) => (html.style.fontSize = `${scale * 100}%`), scale);
	await expect(page.locator('html')).toHaveCSS('font-size', `${root * scale}px`);
	await page.evaluate(() => document.fonts.ready.then(() => undefined));
	await expect(dialog.locator('[data-slot=dialog-title]')).toHaveText(titles[kind]);
	await settle(page);
	return dialog;
}

// This is the future A geometry used only as a composition fixture for B's word atoms.
// B does not ship this viewport, header or Close-float layout.
export async function closeFootprintFixture(page: Page) {
	await page.addStyleTag({
		content: `[data-slot=dialog-content]{width:calc(100vw - 32px)!important;max-width:calc(100vw - 32px)!important;max-height:calc(100dvh - 32px)!important;overflow-y:auto!important;grid-template-columns:minmax(0,1fr)!important}[data-slot=dialog-content]>*,[data-slot=dialog-content] form>*{min-width:0!important}[data-slot=dialog-header]{display:block!important}[data-slot=dialog-header]>*+*{margin-top:.5rem!important}[data-slot=dialog-title]::before{content:'';float:right;width:calc(1.5rem + 4px);height:calc(1em + .5rem)}`
	});
}

export async function wordGeometry(dialog: Locator) {
	await expect(dialog.locator('[data-slot=dialog-title]')).toHaveCSS('overflow-wrap', 'normal');
	const words = await dialog.evaluate((root) => {
		const title = root.querySelector('[data-slot=dialog-title]')!,
			box = title.getBoundingClientRect();
		const tree = document.createTreeWalker(title, NodeFilter.SHOW_TEXT),
			words = [];
		let node: Node | null;
		while ((node = tree.nextNode())) {
			const style = getComputedStyle(node.parentElement!),
				context = document.createElement('canvas').getContext('2d')!;
			context.font = `${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
			for (const match of node.textContent!.matchAll(/\S+/g)) {
				const range = document.createRange();
				range.setStart(node, match.index);
				range.setEnd(node, match.index + match[0].length);
				const rects = [...range.getClientRects()].filter((r) => r.width > 0 && r.height > 0);
				words.push({
					word: match[0],
					minimum: context.measureText(match[0]).width,
					column: box.width,
					lines: rects.length,
					rectangles: rects.map((r) => ({ left: r.left - box.left, right: box.right - r.right }))
				});
			}
		}
		return words;
	});
	await test.info().attach('native-title-word-geometry', {
		body: JSON.stringify(words),
		contentType: 'application/json'
	});
	expect(words.length, 'positive title text').toBeGreaterThan(0);
	for (const word of words) {
		if (word.minimum <= word.column)
			expect(
				word.lines,
				`${word.word} (${word.minimum}px) fits full column ${word.column}px and is never split`
			).toBe(1);
		else
			expect(
				word.lines,
				`${word.word} exceeds the column and wraps inside its span`
			).toBeGreaterThan(1);
		for (const rect of word.rectangles) {
			expect(rect.left, `${word.word} stays inside column left`).toBeGreaterThanOrEqual(-0.5);
			expect(rect.right, `${word.word} stays inside column right`).toBeGreaterThanOrEqual(-0.5);
		}
	}
}

export async function titleText(page: Page, dialog: Locator, expected: string) {
	const title = dialog.locator('[data-slot=dialog-title]');
	expect(await title.textContent(), 'exact heading text, including whitespace').toBe(expected);
	await expect(dialog).toHaveAccessibleName(expected);
	const selected = await title.evaluate((element) => {
		const selection = getSelection()!;
		const range = document.createRange();
		range.selectNodeContents(element);
		selection.removeAllRanges();
		selection.addRange(range);
		const text = selection.toString();
		selection.removeAllRanges();
		return text;
	});
	expect(selected, 'selecting the heading preserves the original text').toBe(expected);
	await expect(title.locator('img,script')).toHaveCount(0);
}

type ClosingSample = {
	text: string | null;
	labelledBy: string | null;
	titleId: string;
	closed: boolean;
};
type DialogEvidenceWindow = Window & {
	__dialogClosing?: { samples: ClosingSample[]; observer: MutationObserver };
};
export async function watchClosingTitle(dialog: Locator) {
	await dialog.evaluate((root) => {
		const title = root.querySelector('[data-slot=dialog-title]')!;
		const sample = () => ({
			text: title.textContent,
			labelledBy: root.getAttribute('aria-labelledby'),
			titleId: title.id,
			closed: root.getAttribute('data-state') === 'closed' || root.hasAttribute('data-closed')
		});
		const samples = [sample()];
		const observer = new MutationObserver(() => {
			if (root.isConnected) samples.push(sample());
		});
		observer.observe(root, {
			subtree: true,
			childList: true,
			characterData: true,
			attributes: true,
			attributeFilter: ['data-state', 'data-closed', 'aria-labelledby']
		});
		(window as DialogEvidenceWindow).__dialogClosing = { samples, observer };
	});
}
export async function closingTitleText(page: Page, expected: string) {
	const samples = await page.evaluate(() => {
		const w = window as DialogEvidenceWindow;
		const state = w.__dialogClosing!;
		state.observer.disconnect();
		delete w.__dialogClosing;
		return state.samples;
	});
	expect(
		samples.some((s) => s.closed),
		'name sampled while the dialog was closing'
	).toBe(true);
	for (const sample of samples) {
		expect(sample.text, 'closing heading text stays unchanged').toBe(expected);
		expect(
			sample.labelledBy?.split(/\s+/),
			'closing accessible label still names the heading'
		).toContain(sample.titleId);
	}
}

export async function settle(page: Page) {
	await page.evaluate(() =>
		Promise.all(
			document
				.getAnimations()
				.filter(
					(a) => a.playState === 'running' && Number.isFinite(a.effect?.getComputedTiming().endTime)
				)
				.map((a) => a.finished.catch(() => undefined))
		)
	);
}

// Isolated text-formatting control in this candidate, not an old whole-app build.
export async function normalTextControl(
	page: Page,
	dialog: Locator,
	priorWrap: 'normal' | 'anywhere' = 'anywhere'
) {
	const capture = () =>
		dialog.evaluate((root) => {
			const box = (node: Element | null) => {
				if (!node) return null;
				const r = node.getBoundingClientRect();
				return { left: r.left, top: r.top, width: r.width, height: r.height };
			};
			return {
				dialog: box(root),
				header: box(root.querySelector('[data-slot=dialog-header]')),
				title: box(root.querySelector('[data-slot=dialog-title]')),
				description: box(root.querySelector('[data-slot=dialog-description]')),
				first: box(
					root.querySelector(
						'input:not([type=hidden]),textarea,select,[data-slot=dialog-footer] button'
					)
				)
			};
		});
	const actual = await capture();
	const control = await page.addStyleTag({
		content: `[data-slot=dialog-title-word]{display:contents!important;overflow-wrap:inherit!important}[data-slot=dialog-title]{overflow-wrap:${priorWrap}!important}`
	});
	let previous;
	try {
		previous = await capture();
	} finally {
		await control.evaluate((e) => e.remove());
	}
	await test.info().attach('normal-title-formatting-comparison', {
		body: JSON.stringify({
			actual,
			previous,
			scope: 'isolated prior text formatting; same candidate/caller',
			priorWrap
		}),
		contentType: 'application/json'
	});
	for (const part of ['dialog', 'header', 'title', 'description', 'first'] as const) {
		expect(actual[part] === null, `${part} presence unchanged`).toBe(previous[part] === null);
		if (!actual[part] || !previous[part]) continue;
		for (const metric of ['left', 'top', 'width', 'height'] as const)
			expect(
				Math.abs(actual[part]![metric] - previous[part]![metric]),
				`100% ${part}.${metric} unchanged`
			).toBeLessThanOrEqual(0.02);
	}
}

export async function titleFlow(
	page: Page,
	kind: TitleCase,
	width: number,
	scale: number,
	root = 16
) {
	const dialog = await openTitle(page, kind, width, scale, root);
	await expect(dialog.locator('[data-slot=dialog-title]')).toHaveCount(1);
	await expect(dialog.locator('[data-slot=dialog-title]')).toHaveRole('heading');
	await titleText(page, dialog, titles[kind]);
	if (scale === 1) await normalTextControl(page, dialog, originalWrap(kind));
	await legacyColumnControl(page, dialog, originalWrap(kind));
	await wordGeometry(dialog);
	await shot(page, `title-b-open-${kind}-${width}-${root * scale}`);
	await watchClosingTitle(dialog);
	await page.keyboard.press('Escape');
	await expect(dialog).toHaveCount(0);
	await closingTitleText(page, titles[kind]);
}

// Existing Dialog/footer intrinsic widths belong to A. B must add no horizontal
// overflow compared with the previous text formatting at the same width/root.
export async function legacyColumnControl(
	page: Page,
	dialog: Locator,
	priorWrap: 'normal' | 'anywhere' = 'anywhere'
) {
	const capture = () =>
		dialog.evaluate((root) => {
			const title = root.querySelector('[data-slot=dialog-title]')!;
			return {
				title: title.getBoundingClientRect().width,
				dialog: root.getBoundingClientRect().width,
				scroll: root.scrollWidth,
				client: root.clientWidth
			};
		});
	const actual = await capture();
	const control = await page.addStyleTag({
		content: `[data-slot=dialog-title-word]{display:contents!important;overflow-wrap:inherit!important}[data-slot=dialog-title]{overflow-wrap:${priorWrap}!important}`
	});
	let previous;
	try {
		previous = await capture();
	} finally {
		await control.evaluate((e) => e.remove());
	}
	await test.info().attach('legacy-column-comparison', {
		body: JSON.stringify({
			actual,
			previous,
			scope: 'prior text formatting only; unchanged Dialog/footer geometry is cut A',
			priorWrap
		}),
		contentType: 'application/json'
	});
	expect(actual.title, 'positive title column').toBeGreaterThan(0);
	expect(
		actual.title,
		'word spans add no column expansion over prior text formatting'
	).toBeLessThanOrEqual(previous.title + 0.02);
	expect(actual.dialog, 'Dialog width unchanged by title text').toBe(previous.dialog);
	expect(
		actual.scroll,
		'word spans add no horizontal scroll over prior text formatting'
	).toBeLessThanOrEqual(previous.scroll + 1);
}
