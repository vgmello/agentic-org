import { expect, test } from '@playwright/test';
import { shot } from './shot';
import { visit } from './ready';
import { attachmentGeometry, focusOutline, inheritsColour, keyboardFocus, kindGeometry, navigation, openChat, sidebarGeometry } from './chat-followups';

for (const width of [320, 640, 960, 1280]) for (const scale of [1, 2]) {
	test(`BUG-94 BUG-70: sidebar icon labels and counts stay together at ${width}/${scale * 100}`, async ({ page }) => {
		await openChat(page, width, scale);
		const nav = await navigation(page);
		await shot(page, `bug94-sidebar-${width}-${scale}`, { context: nav });
		await sidebarGeometry(nav);
	});
}

for (const width of [320, 640, 960, 1280]) for (const scale of [1, 2]) {
test(`BUG-95 BUG-70: kind icon stays beside its word at ${width}/${scale * 100}`, async ({ page }) => {
	await openChat(page, width, scale, '/chat?c=c-general');
	const badge = page.getByTestId('message').filter({ hasText: 'Ready for review: create-org' }).getByTitle('Announcement', { exact: true });
	await kindGeometry(badge);
	await shot(page, `bug95-kind-${width}-${scale}`, { context: badge });
});

test(`BUG-96 BUG-70: sent attachment clip aligns with first filename line at ${width}/${scale * 100}`, async ({ page }) => {
	await openChat(page, width, scale, '/chat?c=c-general');
	const name = 'a-very-long-attachment-filename-that-wraps-across-multiple-lines.txt';
	await page.getByLabel('Attach files').setInputFiles({ name, mimeType: 'text/plain', buffer: Buffer.from('test') });
	await page.getByTestId('composer').fill('Wrapped attachment proof');
	await page.getByTestId('composer').press('Control+Enter');
	const chip = page.getByTestId('message').filter({ hasText: 'Wrapped attachment proof' }).getByRole('list', { name: 'Attachments', exact: true }).getByRole('listitem');
	await expect(chip).toContainText(name);
	await attachmentGeometry(chip, width === 320);
	await chip.scrollIntoViewIfNeeded();
	await shot(page, `bug96-sent-attachment-${width}-${scale}`, { context: chip });
});
}

test('BUG-97 BUG-70: current sidebar icons inherit control colour', async ({ page }) => {
	await openChat(page);
	const nav = await navigation(page), row = nav.getByTestId('needs-you-link');
	await expect(row).toHaveAttribute('aria-current', 'true');
	await inheritsColour(row.locator('svg'), row);
	await shot(page, 'bug97-current-sidebar', { context: row });
});
test('BUG-97 BUG-70: badge and pressed reaction icons inherit control colour', async ({ page }) => {
	await openChat(page, 1280, 1, '/chat?c=c-general');
	const row = page.getByTestId('message').filter({ hasText: 'Ready for review: create-org' });
	const badge = row.getByTitle('Announcement', { exact: true });
	await inheritsColour(badge.locator('svg'), badge);
	await row.getByRole('button', { name: 'Seen, 1', exact: true }).click();
	const chip = row.getByRole('button', { name: 'Seen, 2', exact: true });
	await expect(chip).toHaveAttribute('aria-pressed', 'true');
	await inheritsColour(chip.locator('svg'), chip);
	await shot(page, 'bug97-pressed-reaction', { context: row });
});
test('BUG-97 BUG-70: both reaction states inherit colour', async ({ page }) => {
	await openChat(page, 1280, 1, '/chat?c=c-general');
	const row = page.getByTestId('message').filter({ hasText: 'Ready for review: create-org' });
	let chip = row.getByRole('button', { name: 'Seen, 1', exact: true });
	await expect(chip).toHaveAttribute('aria-pressed', 'false');
	await inheritsColour(chip.locator('svg'), chip);
	await chip.click();
	chip = row.getByRole('button', { name: 'Seen, 2', exact: true });
	await expect(chip).toHaveAttribute('aria-pressed', 'true');
	await inheritsColour(chip.locator('svg'), chip);
	await keyboardFocus(page, chip); await focusOutline(chip);
	await shot(page, 'bug97-98-pressed-focus', { context: row });
});

test('BUG-98 BUG-70: reaction chip has visible keyboard outline', async ({ page }) => {
	await openChat(page, 1280, 1, '/chat?c=c-general');
	const chip = page.getByTestId('message').filter({ hasText: 'Ready for review: create-org' }).getByRole('button', { name: 'Seen, 1', exact: true });
	await keyboardFocus(page, chip); await focusOutline(chip);
	await shot(page, 'bug98-chip-focus', { context: chip });
});
test('BUG-98 BUG-70: every message toolbar action has visible keyboard outline', async ({ page }) => {
	await openChat(page, 1280, 1, '/chat?c=c-general');
	const row = page.getByTestId('message').filter({ hasText: 'Ready for review: create-org' });
	for (const name of ['React Seen', 'React Looking', 'Reply in thread', 'More actions']) {
		const control = row.getByRole('button', { name, exact: true });
		await keyboardFocus(page, control); await focusOutline(control);
		await shot(page, `bug98-toolbar-${name.replaceAll(' ', '-')}`, { context: row });
	}
});

test('BUG-99 BUG-70: attachment removal focuses next then previous then Attach', async ({ page }) => {
	await openChat(page, 640, 1, '/chat?c=c-general');
	await page.getByLabel('Attach files').setInputFiles(['a.txt', 'b.txt', 'c.txt'].map((name) => ({ name, mimeType: 'text/plain', buffer: Buffer.from(name) })));
	const remove = (name: string) => page.getByRole('button', { name: `Remove ${name}`, exact: true });
	await keyboardFocus(page, remove('b.txt')); await remove('b.txt').press('Enter');
	await expect(remove('b.txt')).toHaveCount(0); await expect(remove('c.txt')).toBeFocused();
	await remove('c.txt').press('Enter'); await expect(remove('a.txt')).toBeFocused();
	await remove('a.txt').press('Enter');
	const attach = page.getByRole('button', { name: 'Attach', exact: true });
	await expect(attach).toBeFocused();
	await expect(page.getByRole('list', { name: 'Attached', exact: true })).toHaveCount(0);
	await shot(page, 'bug99-return-attach', { context: attach });
});
test('BUG-99 BUG-70: Attach and Remove have visible keyboard outlines', async ({ page }) => {
	await openChat(page, 1280, 1, '/chat?c=c-general');
	const attach = page.getByRole('button', { name: 'Attach', exact: true });
	await keyboardFocus(page, attach); await focusOutline(attach);
	await page.getByLabel('Attach files').setInputFiles({ name: 'focus.txt', mimeType: 'text/plain', buffer: Buffer.from('test') });
	const remove = page.getByRole('button', { name: 'Remove focus.txt', exact: true });
	await keyboardFocus(page, remove); await focusOutline(remove);
	await shot(page, 'bug99-remove-focus', { context: page.getByRole('list', { name: 'Attached', exact: true }) });
});

test('BUG-99 BUG-70: pointer removal follows first, last and sole attachment identities', async ({ page }) => {
	await openChat(page, 1280, 1, '/chat?c=c-general');
	const names = ['undefined', '__proto__', 'quote" bracket].txt'];
	await page.getByLabel('Attach files').setInputFiles(names.map((name) => ({ name, mimeType: 'text/plain', buffer: Buffer.from(name) })));
	const remove = (name: string) => page.getByRole('button', { name: `Remove ${name}`, exact: true });
	await remove(names[0]).click(); await expect(remove(names[1])).toBeFocused();
	await remove(names[2]).click(); await expect(remove(names[1])).toBeFocused();
	await remove(names[1]).click();
	await expect(page.getByRole('button', { name: 'Attach', exact: true })).toBeFocused();
	await shot(page, 'bug99-pointer-identity');
});

test('BUG-99 BUG-70: changing conversation during removal keeps destination focus', async ({ page }) => {
	await openChat(page, 1280, 1, '/chat?c=c-general');
	await page.getByLabel('Attach files').setInputFiles({ name: 'depart.txt', mimeType: 'text/plain', buffer: Buffer.from('test') });
	await page.evaluate(() => {
		document.querySelector<HTMLButtonElement>('[aria-label="Remove depart.txt"]')!.click();
		const destination = document.querySelector<HTMLButtonElement>('[data-conversation="c-release-v1"]')!;
		destination.focus(); destination.click();
	});
	await expect(page.getByTestId('conversation-title')).toHaveText('#release-v1');
	await expect(page.getByRole('button', { name: 'Attach', exact: true })).not.toBeFocused();
	await expect(page.locator('[data-conversation="c-release-v1"]')).toBeFocused();
	await shot(page, 'bug99-context-switch');
});


test('BUG-99 BUG-70: focus moves only after the removed attachment leaves the DOM', async ({ page }) => {
	await openChat(page, 1280, 1, '/chat?c=c-general');
	await page.getByLabel('Attach files').setInputFiles(['a.txt', 'b.txt'].map((name) => ({ name, mimeType: 'text/plain', buffer: Buffer.from(name) })));
	await page.getByRole('button', { name: 'Remove a.txt', exact: true }).focus();
	await page.evaluate(() => {
		const destination = document.querySelector<HTMLButtonElement>('[aria-label="Remove b.txt"]')!;
		destination.addEventListener('focus', () => destination.setAttribute('data-dom-at-focus', String(document.querySelector('[aria-label="Remove a.txt"]') === null)), { once: true });
	});
	await page.getByRole('button', { name: 'Remove a.txt', exact: true }).press('Enter');
	const destination = page.getByRole('button', { name: 'Remove b.txt', exact: true });
	await expect(destination).toBeFocused();
	await expect(destination).toHaveAttribute('data-dom-at-focus', 'true');
	await shot(page, 'bug99-dom-before-focus', { context: destination });
});

for (const width of [320, 640, 960, 1280]) for (const scale of [1, 2]) {
 test(`BUG-94–99 BUG-70: message render paths preserve geometry at ${width}/${scale * 100}`, async ({ page }) => {
  await openChat(page, width, scale, '/chat?c=c-general');
  for (const [state, url] of [
   ['search', '/chat?q=kind:announcement'],
   ['mentions', '/chat?v=mentions'],
   ['thread', '/chat?c=c-general&thread=m-2'],
   ['bridge', '/chat?c=c-general&surface=bridge'],
   ['stopped', '/chat?c=c-general&chat=offline']
  ]) {
   await visit(page, url);
   const area = state === 'thread' ? page.getByTestId('thread-panel') : page.getByTestId('message-list');
   const rows = area.getByTestId('message');
   expect(await rows.count()).toBeGreaterThan(0);
   const badges = rows.getByTitle('Announcement', { exact: true });
   expect(await badges.count()).toBeGreaterThan(0);
   for (const badge of await badges.all()) await kindGeometry(badge);
   if (state === 'bridge') await expect(area.getByRole('toolbar', { name: 'Message actions' })).toHaveCount(0);
   if (state === 'stopped') for (const control of await area.getByRole('button', { name: /^React / }).all()) await expect(control).toBeDisabled();
   await badges.first().scrollIntoViewIfNeeded();
   await shot(page, `bug95-path-${state}-${width}-${scale}`, { context: badges.first().locator('..') });
  }
  for (const mode of ['muted','digest']) {
   await visit(page,'/chat?c=c-general');
   await page.getByRole('button',{name:'Members',exact:true}).click();
   const members=page.getByTestId('members-panel');
   await members.getByTestId('member-row').filter({hasText:'Sam Okafor'}).getByTestId('mute-select').selectOption(mode);
   await members.getByRole('button',{name:'Close members',exact:true}).click();
   const folded=page.getByTestId(mode==='muted'?'muted-row':'digest-row');
   await expect(folded).toBeVisible();await folded.getByRole('button').first().click();
   const badge=folded.getByTitle('Announcement',{exact:true});await expect(badge).toBeVisible();await kindGeometry(badge);
   await expect(folded.getByRole('toolbar',{name:'Message actions'})).toHaveCount(0);
   await shot(page,`bug95-path-${mode}-${width}-${scale}`,{context:badge.locator('..')});
  }
  for (const place of ['conversation','thread']) {
   await visit(page,`/chat?c=c-general&chat=fail${place==='thread'?'&thread=m-2':''}`);
   const area=place==='thread'?page.getByTestId('thread-panel'):page.getByRole('region',{name:'Conversation',exact:true});
   const name='pending-attachment-with-a-long-wrapping-file-name-for-layout.txt';
   await area.getByLabel('Attach files').setInputFiles({name,mimeType:'text/plain',buffer:Buffer.from('test')});
   await area.getByTestId('composer').fill('Pending attachment layout proof');await area.getByTestId('composer').press('Control+Enter');
   const failed=area.getByTestId('message').filter({hasText:'Pending attachment layout proof'});
   await expect(failed.getByTestId('not-delivered')).toBeVisible();
   await attachmentGeometry(failed.getByRole('list',{name:'Attachments',exact:true}).getByRole('listitem'),width===320);
   await expect(failed.getByRole('toolbar',{name:'Message actions'})).toHaveCount(0);
   await failed.scrollIntoViewIfNeeded();await shot(page,`bug96-path-pending-${place}-${width}-${scale}`,{context:failed});
  }
 });
}

test('BUG-99 BUG-70: thread attachment focus stays in its own composer', async ({ page }) => {
 await openChat(page, 1280, 1, '/chat?c=c-general');
 const conversation = page.getByRole('region', { name: 'Conversation', exact: true });
 await conversation.getByLabel('Attach files').setInputFiles({ name:'next.txt', mimeType:'text/plain', buffer:Buffer.from('main') });
 await page.getByTestId('message-list').getByTestId('message').filter({ hasText:'Ready for review: create-org' }).getByTestId('replies-link').click();
 const pane = page.getByTestId('thread-panel');
 await pane.getByLabel('Attach files').setInputFiles(['first.txt','next.txt'].map(name=>({name,mimeType:'text/plain',buffer:Buffer.from(name)})));
 await keyboardFocus(page, pane.getByRole('button', {name:'Remove first.txt',exact:true}));
 await pane.getByRole('button', {name:'Remove first.txt',exact:true}).press('Enter');
 await expect(pane.getByRole('button', {name:'Remove next.txt',exact:true})).toBeFocused();
 await expect(conversation.getByRole('button', {name:'Remove next.txt',exact:true})).not.toBeFocused();
 await pane.getByRole('button', {name:'Remove next.txt',exact:true}).press('Enter');
 await expect(pane.getByRole('button', {name:'Attach',exact:true})).toBeFocused();
 await shot(page,'bug99-thread-scoped-focus',{context:pane});
});


test('BUG-95 BUG-70: metadata threshold has two sides at 16, 32 and 40px roots', async ({ page }) => {
 const metadataThresholdRem = 10.765625; // Independent boundary golden; derivation guarded in message-layout.test.ts.
 await openChat(page, 1280, 1, '/chat?c=c-general');
 const badge = page.getByTestId('message').filter({ hasText:'Ready for review: create-org' }).getByTitle('Announcement',{exact:true});
 for (const root of [16,32,40]) for (const delta of [-1,1]) {
  await page.evaluate(({root,width})=>{
   document.documentElement.style.setProperty('font-size',`${root}px`,'important');
   document.querySelector('[data-id="m-2"]')!.parentElement!.style.width=`${width}px`;
  },{root,width:root*metadataThresholdRem+delta});
  const layout = await badge.evaluate(e=>{
   const article=e.closest('article')!, avatar=article.firstElementChild!.firstElementChild!.getBoundingClientRect();
   const header=e.parentElement!.getBoundingClientRect(), body=article.children[2].getBoundingClientRect();
   return { below:header.top>=avatar.bottom-.5 && Math.abs(header.left-avatar.left)<=.5, beside:header.left>=avatar.right-.5 && header.top<avatar.bottom, bodyInColumn:body.left>=avatar.right-.5, overflow:article.scrollWidth-article.clientWidth };
  });
  expect(layout).toMatchObject(delta<0?{below:true,bodyInColumn:true}:{beside:true,bodyInColumn:true});
  expect(layout.overflow).toBeLessThanOrEqual(.5);
  await kindGeometry(badge);
  await shot(page,`bug95-threshold-${root}-${delta}`,{context:badge.locator('..')});
 }
});

test('BUG-95 BUG-70: wide 1280/100 positions match the original flex column', async ({ page }) => {
 await openChat(page,1280,1,'/chat?c=c-general');
 const row=page.getByTestId('message').filter({hasText:'Ready for review: create-org'});
 const difference=await row.evaluate(article=>{
  const avatar=article.firstElementChild!, body=article.children[2] as HTMLElement, header=article.children[1] as HTMLElement;
  const targets=[avatar.firstElementChild!,header,body.querySelector('[data-testid="message-body"]')!];
  const boxes=()=>targets.map(e=>{const r=e.getBoundingClientRect();return [r.left,r.top,r.width,r.height];});
  const before=boxes();
  // Reconstruct the original sibling avatar + flex-1 column containing metadata and body.
  body.prepend(header);body.style.flex='1';(article as HTMLElement).style.display='flex';(avatar as HTMLElement).style.flexShrink='0';
  const after=boxes();return before.map((r,i)=>r.map((v,j)=>Math.abs(v-after[i][j])));
 });
 for(const box of difference) for(const value of box) expect(value).toBeLessThanOrEqual(.5);
 await shot(page,'bug95-wide-original-layout',{context:row});
});


for (const width of [320,640,960,1280]) test(`BUG-98/99 BUG-70: outlines remain visible at ${width}/200`, async ({ page }) => {
 await openChat(page,width,2,'/chat?c=c-general');
 const row=page.getByTestId('message').filter({hasText:'Ready for review: create-org'});
 for (const name of ['Seen, 1','React Seen','React Looking','Reply in thread','More actions']) {
  const control=row.getByRole('button',{name,exact:true});
  await keyboardFocus(page,control);await focusOutline(control);
 }
 await shot(page,`bug98-focus-200-${width}`,{context:row});
 const attach=page.getByRole('button',{name:'Attach',exact:true});
 await keyboardFocus(page,attach);await focusOutline(attach);
 await page.getByLabel('Attach files').setInputFiles({name:'visible-focus.txt',mimeType:'text/plain',buffer:Buffer.from('test')});
 const remove=page.getByRole('button',{name:'Remove visible-focus.txt',exact:true});
 await keyboardFocus(page,remove);await focusOutline(remove);
 await shot(page,`bug99-focus-200-${width}`,{context:page.getByRole('list',{name:'Attached',exact:true})});
});


test('BUG-94 BUG-70: geometry oracle rejects an overlapping count inside row bounds', async ({ page }) => {
 await openChat(page,1280,2);
 const nav=await navigation(page);
 await sidebarGeometry(nav);
 await nav.getByTestId('needs-you-link').evaluate(row=>{
  const label=row.lastElementChild!.firstElementChild!.getBoundingClientRect(), box=row.getBoundingClientRect();
  (row as HTMLElement).style.position='relative';
  const count=row.querySelector<HTMLElement>('[data-testid="needs-you-count"]')!;
  Object.assign(count.style,{position:'absolute',left:`${label.left-box.left}px`,top:`${label.top-box.top}px`});
 });
 await expect(sidebarGeometry(nav)).rejects.toThrow(/noOverlap/);
});
