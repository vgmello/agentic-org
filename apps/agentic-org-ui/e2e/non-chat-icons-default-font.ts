import { expect, test } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { create, iconIs, removeIs, focusTarget } from './non-chat-icons';

// This file is selected only by the Chromium project with a 20px browser default.
for (const width of [320, 640, 960, 1280])
	for (const screen of ['projects', 'profile'] as const) {
		test(`E3 PROJ-2/3 TEAM-8 BUG-70: ${screen} icons at ${width}px with a real 20px default font`, async ({
			page
		}) => {
			await create(page, width);
			await visit(page, screen === 'profile' ? '/projects/beacon' : '/projects');
			await expect(page.locator('html')).toHaveCSS('font-size', '20px');
			if (screen === 'profile')
				await iconIs(
					page.getByRole('navigation', { name: 'Breadcrumb', exact: true }).locator('svg'),
					'mini:chevron-right'
				);
			const remove = page.getByRole('button', { name: 'Remove Beacon from Web', exact: true });
			await removeIs(remove, 'Remove Beacon from Web');
			await focusTarget(page, remove);
			await shot(page, `non-chat-${screen}-default20-${width}`, { context: remove });
		});
	}
