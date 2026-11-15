import { expect, type Page } from '@playwright/test';

/** Follow the visible View menu; commands retain the original chart state assertions. */
export async function viewAction(page: Page, name: string) {
	await page.getByRole('button', { name: 'View', exact: true }).click();
	const action = page.getByRole('menuitem', { name, exact: true });
	await expect(action).toBeVisible();
	await action.click();
}

export async function expectViewAction(page: Page, name: string) {
	await page.getByRole('button', { name: 'View', exact: true }).click();
	await expect(page.getByRole('menuitem', { name, exact: true })).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(page.getByRole('menu')).toHaveCount(0);
}
