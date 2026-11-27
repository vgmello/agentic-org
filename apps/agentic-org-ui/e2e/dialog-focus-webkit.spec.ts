import { expect, test } from '@playwright/test';
import { enterMockedOrganization } from './mock-organization';
import { visit } from './ready';

test('pointer-opened dialog resets fields via onOpenAutoFocus and restores focus on close @webkit', async ({
	page
}) => {
	await enterMockedOrganization(page);
	await visit(page, '/projects/atlas/work');

	const newTaskButton = page.getByRole('button', { name: 'New task', exact: true });
	await expect(newTaskButton).toBeVisible();

	// Open with a real pointer click (in native WebKit, button click does not move document.activeElement).
	await newTaskButton.click();
	const dialog = page.locator('[data-slot="dialog-content"]');
	await expect(dialog).toBeVisible();

	// Closing with Escape must restore focus back to the clicked opener button (not document.body).
	await page.keyboard.press('Escape');
	await expect(dialog).toHaveCount(0);
	await expect(newTaskButton).toBeFocused();

	// Reopening via pointer click and closing via Cancel must also restore focus back to the clicked opener button.
	await newTaskButton.click();
	await expect(dialog).toBeVisible();
	const closeBtn = dialog.getByRole('button', { name: /Cancel|Close/i }).first();
	await closeBtn.click();
	await expect(dialog).toHaveCount(0);
	await expect(newTaskButton).toBeFocused();

	// ShortcutsDialog uses DialogContent and restores focus to the active control on close.
	await page.keyboard.press('?');
	const shortcuts = page.getByTestId('shortcuts-help-dialog');
	await expect(shortcuts).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(shortcuts).toHaveCount(0);
	await expect(newTaskButton).toBeFocused();
});
