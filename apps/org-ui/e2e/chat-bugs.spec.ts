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

test('CHAT-5/6 BUG-41: stopped office keeps asks and approvals read-only everywhere', async ({page}) => {
 await open(page, '?chat=offline');
 await expect(page.getByTestId('office-down')).toBeVisible();
 for (const card of await page.getByTestId('ask-card').all()) {
  await expect(card.getByRole('button')).toHaveCount(0);
  await expect(card).toContainText("The office isn't running. Restart it from Offices.");
 }
 await shot(page, 'chat-offline-needs-you');
 await go(page, 'atlas');
 const asks = page.getByTestId('ask-card');
 await expect(asks.first()).toBeVisible();
 for (const card of await asks.all()) await expect(card.getByRole('button')).toHaveCount(0);
 await shot(page, 'chat-offline-conversation');
});

test('CHAT-7/8 BUG-42: incoming conversation and thread replies are in polite message logs', async ({page}) => {
 await open(page);
 await go(page, 'Inês Duarte');
 const log = page.getByRole('log', { name:'Messages', exact:true });
 await expect(log).toHaveAttribute('aria-live','polite');
 const composer = page.getByTestId('composer');
 await composer.fill('Could you help?');
 await composer.press('Control+Enter');
 await expect(log).toContainText('Sample reply');
 await expect(composer).toBeFocused();
 await shot(page, 'chat-announced-reply');
 await go(page, 'release-v1');
 await page.getByTestId('replies-link').first().click();
 const thread = page.getByTestId('thread-panel');
 const replies = thread.getByRole('log', {name:'Thread messages'});
 await expect(replies).toHaveAttribute('aria-live','polite');
 await thread.getByTestId('composer').fill('An accessible thread reply');
 await thread.getByTestId('composer').press('Control+Enter');
 await expect(replies).toContainText('An accessible thread reply');
 await shot(page, 'chat-announced-thread-reply');
});

for (const action of ['option', 'reply', 'reject'] as const) {
 test(`CHAT-5 BUG-43: keyboard focus remains useful after ${action}`, async ({page}) => {
  await open(page);
  const card = page.getByTestId('ask-card').filter({hasText:action === 'reject' ? 'Publish the 0.2' : 'Block bug reports'});
  if (action !== 'option') {
   await card.getByRole('button',{name:action==='reject'?'Reject…':'Reply…'}).click();
   await card.getByRole('textbox').fill('Please wait for the audit.');
  }
  const button = card.getByRole('button',{name:action==='option'?'Yes, block':action==='reject'?'Reject':'Send reply',exact:true});
  await button.focus();
  await button.press('Enter');
  await expect(card).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => document.activeElement?.tagName)).not.toBe('BODY');
  const focused = page.locator(':focus');
  await expect(page.getByTestId('needs-you-list')).toContainText('asks you');
  await expect(focused).toHaveRole('button');
  await shot(page, `chat-focus-${action}`);
  // Answer the final card: focus lands on the empty result rather than the document.
  const remaining = page.getByTestId('ask-card');
  const answer = remaining.getByRole('button',{name:action==='reject'?'Yes, block':'Approve',exact:true});
  await answer.focus(); await answer.press('Enter');
  await expect(page.getByTestId('nothing-needs-you')).toBeFocused();
 });
}

test('CHAT-14 BUG-43: keyboard Retry focuses the delivered message', async ({page}) => {
 await open(page, '?chat=fail'); await go(page, 'general');
 await page.getByTestId('composer').fill('Retry focus target');
 await page.getByTestId('composer').press('Control+Enter');
 const retry = page.getByRole('button',{name:'Retry',exact:true});
 await expect(retry).toBeVisible(); await retry.focus(); await retry.press('Enter');
 await expect(page.getByTestId('not-delivered')).toHaveCount(0);
 const delivered=page.getByTestId('message').filter({hasText:'Retry focus target'});
 await expect(delivered).toHaveCount(1); await expect(delivered).toBeFocused();
 await shot(page, 'chat-focus-delivered');
});

test('CHAT-1/8/15 BUG-44: channel and thread survive Back, Forward and reload without losing mock parameters', async ({page}) => {
 await open(page, '?chat=fail&surface=bridge');
 await go(page, 'general');
 await expect(page).toHaveURL(/c=c-general/);
 await go(page, 'release-v1');
 await expect(page).toHaveURL(/c=c-release-v1/);
 await page.getByTestId('replies-link').first().click();
 await expect(page).toHaveURL(/thread=/);
 const threadURL=page.url();
 await page.reload();
 await expect(page.getByTestId('conversation-title')).toHaveText('#release-v1');
 await expect(page.getByTestId('thread-panel')).toContainText('Storage is done');
 expect(new URL(page.url()).searchParams.get('chat')).toBe('fail');
 expect(new URL(page.url()).searchParams.get('surface')).toBe('bridge');
 await page.getByRole('button',{name:'Close thread'}).click();
 await expect(page).not.toHaveURL(/thread=/);
 await page.goBack();
 await expect(page).toHaveURL(threadURL);
 await expect(page.getByTestId('thread-panel')).toBeVisible();
 await page.goBack();
 await expect(page.getByTestId('thread-panel')).toBeHidden();
 await page.goBack();
 await expect(page.getByTestId('conversation-title')).toHaveText('#general');
 await page.goForward();
 await expect(page.getByTestId('conversation-title')).toHaveText('#release-v1');
 await shot(page, 'chat-history-restored');
});
