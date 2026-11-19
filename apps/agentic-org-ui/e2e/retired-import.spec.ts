import { expect, test } from '@playwright/test';
import { shot } from './shot';
import { enterMockedOrganization } from './mock-organization';

test(
	'IMP-retired: Hire remains the way in while Add agent, existing-team import and native window controls are absent',
	{ tag: '@journey' },
	async ({ page }) => {
		await enterMockedOrganization(page, 'Fresh organization');
		// Hiring is the only way an agent comes to exist (our-org-in-the-app §7): Members links to Hire, and offers no Add agent.
		await expect(page.getByRole('link', { name: 'Hire an agent', exact: true })).toHaveAttribute(
			'href',
			'/teams/hire'
		);
		await expect(page.getByRole('button', { name: 'Add agent', exact: true })).toHaveCount(0);
		await expect(page.getByRole('button', { name: /bring in/i })).toHaveCount(0);
		await expect(page.locator('[data-tauri-drag-region]')).toHaveCount(0);
		// Org chart is a Teams view now (offices-and-teams-ux §7), reached from the Teams view switch.
		const orgChart = page
			.getByRole('navigation', { name: 'Teams views', exact: true })
			.getByRole('link', { name: 'Org chart', exact: true });
		await orgChart.click();
		await expect(page).toHaveURL(/\/teams\/org-chart$/);
		await expect(orgChart).toHaveAttribute('aria-current', 'page');
		await expect(page).toHaveTitle('Org chart · Teams · Agentic Org');
		await expect(page.getByRole('button', { name: /bring in/i })).toHaveCount(0);
		await shot(page, 'IMP-retired-org-chart');
	}
);
