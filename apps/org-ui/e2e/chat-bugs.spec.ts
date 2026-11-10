import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function open(page: Page, query = '') {
 await visit(page, '/create');
 await page.getByLabel('Organization name').fill('Acme');
 await page.getByRole('button', { name: 'Create organization' }).click();
 await expect(page).toHaveURL(/\/agents$/);
 await visit(page, `/chat${query}`);
 await expect(page.getByRole('navigation', { name: 'Chat' })).toBeVisible();
}
const side = (page: Page) => page.getByRole('navigation', { name: 'Chat' });
const go = (page: Page, name: string) => side(page).getByTestId('conversation-link').filter({ hasText: name }).first().click();

for (const panel of ['thread', 'members'] as const) {
 test(`CHAT-1/8 BUG-40: ${panel} overlays without squeezing the conversation at 960x640`, async ({ page }) => {
  await open(page);
  await go(page, 'release-v1');
  if (panel === 'thread') await page.getByTestId('replies-link').first().click();
  else await page.getByRole('button', { name: 'Members', exact: true }).click();
  await page.setViewportSize({width:960,height:640});
  const pane = page.getByTestId(`${panel}-panel`);
  await expect(pane).toBeVisible();
  await shot(page, `chat-${panel}-minimum`);
  const composer = page.getByRole('region', { name:'Conversation', exact:true }).getByTestId('composer');
  await expect.poll(async () => (await composer.boundingBox())?.width ?? 0).toBeGreaterThan(300);
  const bounds = await pane.boundingBox();
  expect(bounds).not.toBeNull();
  expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(960);
  expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(640);
  await pane.getByRole('button', {name: `Close ${panel}`}).click();
  await expect(pane).toBeHidden();
  const search = page.getByRole('searchbox', {name:'Search chat'});
  const searchBounds = await search.boundingBox();
  expect(searchBounds!.x + searchBounds!.width).toBeLessThanOrEqual(960);
  await composer.fill('Still room to write');
  await composer.press('Control+Enter');
  await expect(page.getByTestId('message-list')).toContainText('Still room to write');
  await shot(page, `chat-${panel}-minimum-closed`);
 });
}
