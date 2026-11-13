import { expect, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function create(page: Page, width: number, scale = 1) {
	await page.setViewportSize({ width, height: 900 });
	await page.addInitScript((scale) => {
		const apply = () => document.documentElement.style.setProperty('font-size', `${scale * 100}%`, 'important');
		addEventListener('DOMContentLoaded', apply, { once: true });
		if (document.documentElement) apply();
	}, scale);
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization', exact: true }).click();
	await expect(page).toHaveURL(/\/agents$/);
	// URL changes before the destination is ready; avoid interrupting the create handoff (BUG-93).
	await expect(page.getByRole('main', { name: 'Agents', exact: true })).toBeVisible();
	await expect(page.getByRole('table', { name: 'Agents', exact: true })).toBeVisible();
}

export async function openChat(page: Page, width = 1280, scale = 1, url = '/chat') {
	await create(page, width, scale);
	await visit(page, url);
	await expect(page.getByTestId('conversation-title')).toBeAttached();
	await expect(page.getByRole('region', { name: 'Conversation', exact: true })).toBeVisible();
	await page.evaluate(() => document.fonts.ready);
}
export async function navigation(page: Page) {
	const nav = page.getByRole('navigation', { name: 'Chat', exact: true });
	if (!(await nav.isVisible())) await page.getByRole('button', { name: /^Conversations(?:,|$)/ }).click();
	await expect(nav).toBeVisible();
	return nav;
}
export async function inheritsColour(icon: Locator, control: Locator) {
	await expect(icon).toHaveCount(1);
	await expect(icon).toHaveCSS('color', await control.evaluate((e) => getComputedStyle(e).color));
}
export async function keyboardFocus(page: Page, control: Locator) {
	await page.keyboard.press('Tab');
	await control.focus();
	await expect(control).toBeFocused();
}
export async function focusOutline(control: Locator) {
	await expect(control).toBeFocused();
	await expect.poll(() => control.evaluate((e) => {
		const style = getComputedStyle(e);
		return { visible: e.matches(':focus-visible'), width: parseFloat(style.outlineWidth), offset: parseFloat(style.outlineOffset), style: style.outlineStyle, colour: style.outlineColor, expected: getComputedStyle(document.documentElement).getPropertyValue('--color-indigo-400').trim() };
	})).toMatchObject({ visible: true, width: 2, offset: 2, style: 'solid' });
	// Resolve the actual design token through CSS rather than comparing an OKLCH string to RGB.
	await expect.poll(() => control.evaluate((e) => {
		const probe = document.createElement('span'); probe.style.color = 'var(--color-indigo-400)'; e.append(probe);
		const matches = getComputedStyle(e).outlineColor === getComputedStyle(probe).color; probe.remove(); return matches;
	})).toBe(true);
}
export async function sidebarGeometry(nav: Locator) {
 const navPadding=await nav.evaluate(e=>({actual:parseFloat(getComputedStyle(e).paddingLeft),expected:Math.min(parseFloat(getComputedStyle(document.documentElement).fontSize)*.75,12)}));
 expect(navPadding.actual).toBeCloseTo(navPadding.expected,1);
	const rows = nav.locator('[data-testid="needs-you-link"], [data-testid="mentions-link"], [data-testid="threads-link"], [data-testid="conversation-link"]');
	expect(await rows.count()).toBeGreaterThan(5);
	for (const row of await rows.all()) {
		if (!(await row.isVisible())) continue;
		const measured = await row.evaluate((e) => {
			const box = e.getBoundingClientRect(), visual = e.firstElementChild!, icon = visual.getBoundingClientRect();
			const content = e.lastElementChild!, label = content.firstElementChild!, range = document.createRange(); range.selectNodeContents(label);
			const lines = [...range.getClientRects()].filter((r) => r.width > 0), first = lines[0];
			const counts = [...content.children].slice(1).map((c) => c.getBoundingClientRect());
			const words = [...label.textContent!.matchAll(/\S+/g)].map((word) => {
				const wordRange = document.createRange(); wordRange.setStart(label.firstChild!, word.index!); wordRange.setEnd(label.firstChild!, word.index! + word[0].length);
				return { word: word[0], fragments: [...wordRange.getClientRects()].filter((r) => r.width > 0).length };
			});
			const root = parseFloat(getComputedStyle(document.documentElement).fontSize);
   const fixed = e.getAttribute('data-testid') !== 'conversation-link';
   const probe=document.createElement('span');Object.assign(probe.style,{position:'absolute',display:'inline-block',width:'6ch'});label.append(probe);
   const minimum=probe.getBoundingClientRect().width;probe.remove();
   const labelStyle=getComputedStyle(label);
   const readable=label.getBoundingClientRect().width>0 && (!fixed || label.getBoundingClientRect().width>=minimum-.5);
			return { name: e.textContent, words, fixed, readable, fixedWordStyle: !fixed || (labelStyle.wordBreak==='normal' && labelStyle.overflowWrap==='normal'), lines: lines.length,
				iconBefore: !!first && icon.right <= first.left + 1, sharesLine: !!first && icon.top < first.bottom && icon.bottom > first.top,
				fits: [...lines, ...counts].every((r) => r.left >= icon.right - 1 && r.right <= box.right + 1),
				noOverlap: counts.every((c) => lines.every((r) => c.right <= r.left + .5 || c.left >= r.right - .5 || c.bottom <= r.top + .5 || c.top >= r.bottom - .5)),
				iconSize: visual.tagName.toLowerCase() === 'svg' ? { width: icon.width, height: icon.height, expected: Math.min(root * 1.5, 32) } : undefined };
		});
		expect(measured.lines).toBeGreaterThan(0);
		expect(measured, measured.name ?? '').toMatchObject({ iconBefore: true, sharesLine: true, fits: true, noOverlap: true, readable: true, fixedWordStyle: true });
		if (measured.iconSize) {
			expect(measured.iconSize.width).toBeCloseTo(measured.iconSize.expected, 1);
			expect(measured.iconSize.height).toBeCloseTo(measured.iconSize.expected, 1);
		}
		if (measured.fixed) {
			expect(measured.words.length).toBeGreaterThan(0);
			for (const word of measured.words) expect(word.fragments, `${measured.name}: whole word ${word.word}`).toBe(1);
		}
	}
}

export async function kindGeometry(badge: Locator) {
	const geometry = await badge.evaluate((e) => {
		const icon = e.querySelector('svg')!.getBoundingClientRect(), walker = document.createTreeWalker(e, NodeFilter.SHOW_TEXT);
		let word: DOMRect | undefined;
		for (let n = walker.nextNode(); n; n = walker.nextNode()) if (n.textContent?.includes('Announcement')) { const range = document.createRange(); range.selectNode(n); word = range.getBoundingClientRect(); break; }
		const unit = e.querySelector('span')!.getBoundingClientRect(), column = e.parentElement!, available = column.getBoundingClientRect();
		return { wordFound: !!word, beside: !!word && icon.right <= word.left + 1 && icon.top < word.bottom && icon.bottom > word.top, centred: !!word && Math.abs((icon.top + icon.bottom) / 2 - (word.top + word.bottom) / 2) <= 2, fits: unit.left >= available.left - 1 && unit.right <= Math.min(available.right, innerWidth) + .5 && column.scrollWidth <= column.clientWidth + .5, measured: { left: unit.left, right: unit.right, width: unit.width, availableRight: available.right, viewport: innerWidth, clientWidth: column.clientWidth, scrollWidth: column.scrollWidth } };
	});
	expect(geometry, JSON.stringify(geometry)).toMatchObject({ wordFound: true, beside: true, centred: true, fits: true });
}
export async function attachmentGeometry(chip: Locator, wraps = false) {
	const geometry = await chip.evaluate((e) => {
		const icon = e.querySelector('svg')!.getBoundingClientRect(), range = document.createRange(); range.selectNodeContents(e.querySelector('span')!);
		const lines = [...range.getClientRects()];
		const span = e.querySelector('span')!, spanBox = span.getBoundingClientRect(), lineHeight = parseFloat(getComputedStyle(span).lineHeight);
		return { lineBoxOffset: Math.abs(icon.top - (spanBox.top + (lineHeight - icon.height) / 2)), lines: lines.length, firstLineOffset: Math.abs((icon.top + icon.bottom) / 2 - (lines[0].top + lines[0].bottom) / 2), scale: parseFloat(getComputedStyle(document.documentElement).fontSize) / 16 };
	});
	expect(geometry.lines).toBeGreaterThan(wraps ? 1 : 0);
	expect(geometry.firstLineOffset).toBeLessThanOrEqual(4 * geometry.scale);
	expect(geometry.lineBoxOffset).toBeLessThanOrEqual(.5);
}

/** Read actual glyph rectangles, including bare text before a content-column fix. */
export async function labelGeometry(control: Locator, boundary: Locator = control) {
 const bounds = await boundary.boundingBox(); expect(bounds).not.toBeNull();
 const measured = await control.evaluate((e, box) => {
  const rect=e.getBoundingClientRect(), style=getComputedStyle(e), walker=document.createTreeWalker(e,NodeFilter.SHOW_TEXT);
  const lines: DOMRect[]=[], words: {text:string; fragments:number}[]=[];
  for(let n=walker.nextNode();n;n=walker.nextNode()) {
   if(!n.textContent?.trim()) continue;
   const range=document.createRange();range.selectNodeContents(n);lines.push(...[...range.getClientRects()].filter(r=>r.width>0));
   for(const word of n.textContent.matchAll(/\S+/g)) {
    const r=document.createRange();r.setStart(n,word.index!);r.setEnd(n,word.index!+word[0].length);
    words.push({text:word[0],fragments:[...r.getClientRects()].filter(b=>b.width>0).length});
   }
  }
  const probe=document.createElement('span');Object.assign(probe.style,{position:'absolute',width:'6ch'});e.append(probe);const sixCh=probe.getBoundingClientRect().width;Object.assign(probe.style,{width:'max-content',whiteSpace:'nowrap',font:style.font});probe.textContent=e.textContent?.trim()??'';const natural=probe.getBoundingClientRect().width;probe.remove();
  return {width:rect.width,minimum:Math.min(sixCh,natural),lines:lines.length,words,
   fits:lines.every(r=>r.left>=Math.max(rect.left,box.x)-.5 && r.right<=Math.min(rect.right,box.x+box.width,innerWidth)+.5 && r.top>=rect.top-.5 && r.bottom<=rect.bottom+.5),
   target: rect.height>=parseFloat(getComputedStyle(document.documentElement).fontSize)*2-.5,
   whiteSpace:style.whiteSpace};
 }, bounds!);
 expect(measured.width).toBeGreaterThan(0);expect(measured.width).toBeGreaterThanOrEqual(measured.minimum-.5);
 expect(measured.lines).toBeGreaterThan(0);expect(measured,JSON.stringify(measured)).toMatchObject({fits:true});
 for(const word of measured.words) expect(word.fragments,`whole fixed word ${word.text}`).toBe(1);
 return measured;
}
export async function headerGeometry(page: Page) {
 const region=page.getByRole('region',{name:'Conversation',exact:true}), header=region.locator(':scope > div').first();
 const measured=await header.evaluate(e=>({width:e.clientWidth,scroll:e.scrollWidth}));
 expect(measured.width).toBeGreaterThan(0);expect(measured.scroll).toBeLessThanOrEqual(measured.width+1);
 await labelGeometry(page.getByTestId('conversation-title'),header);
 const toggle=header.getByRole('button',{name:/^Conversations(?:,|$)/});
 if(await toggle.count()) await labelGeometry(toggle,header);
 const search=page.getByRole('searchbox',{name:'Search chat',exact:true});
 const searchGeometry=await search.evaluate(e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e),p=document.createElement('span');Object.assign(p.style,{position:'absolute',font:s.font,width:'6ch'});document.body.append(p);const min=p.getBoundingClientRect().width;p.remove();return {width:r.width-parseFloat(s.paddingLeft)-parseFloat(s.paddingRight),min,right:r.right,left:r.left};});
 expect(searchGeometry.width).toBeGreaterThanOrEqual(searchGeometry.min-.5);expect(searchGeometry.left).toBeGreaterThanOrEqual(0);expect(searchGeometry.right).toBeLessThanOrEqual(await page.evaluate(()=>innerWidth));
}
export async function creationGeometry(nav: Locator) {
 for(const name of ['New message','Add channel']) {
  const button=nav.getByRole('button',{name,exact:true});await labelGeometry(button,nav);
  await labelGeometry(button.locator(':scope > span'),button);
  const geometry=await button.evaluate(e=>{
   const icon=e.querySelector('svg')!.getBoundingClientRect(),walker=document.createTreeWalker(e,NodeFilter.SHOW_TEXT);let first:DOMRect|undefined;
   for(let n=walker.nextNode();n;n=walker.nextNode()) if(n.textContent?.trim()){const r=document.createRange(),start=n.textContent.search(/\S/),end=n.textContent.slice(start).search(/\s/);r.setStart(n,start);r.setEnd(n,end<0?n.textContent.length:start+end);first=r.getBoundingClientRect();break;}
   return {beside:!!first&&icon.right<=first.left+.5&&icon.top<first.bottom&&icon.bottom>first.top,size:icon.width,expected:Math.min(parseFloat(getComputedStyle(document.documentElement).fontSize)*1.5,32)};
  });
  expect(geometry).toMatchObject({beside:true});expect(geometry.size).toBeCloseTo(geometry.expected,1);
 }
}
export async function announcementGeometry(page: Page, area: Locator = page.getByTestId('message-list')) {
 const row=area.getByTestId('message').filter({hasText:'Ready for review: create-org'}),body=row.getByTestId('message-body'),link=body.getByRole('link',{name:'the branch',exact:true});
 await expect(link).toHaveAttribute('href','https://example.com/branch/create-org');
 const measured=await link.evaluate(e=>{
  const body=e.closest('[data-testid="message-body"]')!,b=body.getBoundingClientRect(),r=document.createRange();r.selectNodeContents(e);
  return {width:b.width,fragments:[...r.getClientRects()].filter(r=>r.width>0).map(r=>({left:r.left,right:r.right})),left:b.left,right:Math.min(b.right,innerWidth)};
 });
 expect(measured.width).toBeGreaterThan(0);expect(measured.fragments.length).toBeGreaterThan(0);
 for(const r of measured.fragments){expect(r.left).toBeGreaterThanOrEqual(measured.left-.5);expect(r.right).toBeLessThanOrEqual(measured.right+.5);}
}

export async function optionGeometry(card: Locator) {
 const approval=(await card.textContent())?.includes('Publish the 0.2');
 const labels=approval?['Approve','Reject…']:['Yes, block','No, allow (default)'];
 for(const name of labels) {
  const button=card.getByRole('button',{name,exact:true});const geometry=await labelGeometry(button,card);
  expect(geometry.target).toBe(true);await expect(button).toHaveCSS('white-space','normal');
  const size=await button.boundingBox();expect(size!.height).toBeGreaterThanOrEqual((await button.page().viewportSize())!.width<640?44:24);
  await expect(button).toHaveCSS('text-align','center');
 }
 // BUG-160: at the default text size an option matches Reply… (same padding; same height while its
 // label fits one line). Larger text keeps the option's 8px padding cap so long labels still fit.
 const reply=card.getByRole('button',{name:'Reply…',exact:true});
 const root=await card.evaluate(()=>parseFloat(getComputedStyle(document.documentElement).fontSize));
 if(!approval&&root===16&&await reply.isVisible()) {
  const style=(e:Element)=>{const s=getComputedStyle(e),r=e.getBoundingClientRect(),line=parseFloat(s.lineHeight);return {height:r.height,padding:s.paddingLeft,lines:Math.round((r.height-parseFloat(s.paddingTop)-parseFloat(s.paddingBottom)-parseFloat(s.borderTopWidth)-parseFloat(s.borderBottomWidth))/line)};};
  const r=await reply.evaluate(style);
  for(const name of labels) {
   const o=await card.getByRole('button',{name,exact:true}).evaluate(style);
   expect(o.padding,`${name} padding matches Reply…`).toBe(r.padding);
   if(o.lines<=1&&(await card.page().viewportSize())!.width>=640) expect(Math.abs(o.height-r.height),`${name} height matches Reply…`).toBeLessThanOrEqual(.5);
  }
 }
}
/** Same native flow is reused by the actual Chromium20px default-font project. */
export async function largeTextFlow(page: Page, width: number, scale: number) {
 await openChat(page,width,scale,'/chat?c=c-general');
 await headerGeometry(page);await announcementGeometry(page);await shot(page,`bug134-header-${width}-${scale}`);
 const nav=await navigation(page);await creationGeometry(nav);
 for(const name of ['Add channel','New message']) {
  const button=nav.getByRole('button',{name,exact:true});await button.scrollIntoViewIfNeeded();
  await shot(page,`bug136-${name.replaceAll(' ','-')}-${width}-${scale}`,{context:button});
 }
 if(await page.getByRole('dialog',{name:'Conversations',exact:true}).isVisible()) await page.keyboard.press('Escape');
 for(const [state,url] of [['search','/chat?q=kind:announcement'],['empty-search','/chat?q=no-such-message-134'],['mentions','/chat?v=mentions'],['threads','/chat?v=threads']]) {
  await visit(page,url);await headerGeometry(page);await shot(page,`bug134-${state}-${width}-${scale}`);
  if(state==='search') await announcementGeometry(page);
  if(state==='empty-search') await expect(page.getByTestId('no-results')).toHaveText('Nothing matches.');
 }
 for(const [state,url] of [['needs','/chat'],['conversation','/chat?c=c-project-atlas'],['search','/chat?q=has:ask']]) {
  await visit(page,url);await headerGeometry(page);
  for(const phrase of ['Block bug reports','Publish the 0.2']) {
   const card=page.getByTestId('ask-card').filter({hasText:phrase});await optionGeometry(card);
   const button=card.getByRole('button',{name:phrase==='Block bug reports'?'No, allow (default)':'Reject…',exact:true});
   await keyboardFocus(page,button);await focusOutline(button);
   await shot(page,`bug135-${state}-${phrase.startsWith('Block')?'ask':'approval'}-${width}-${scale}`,{context:button});
  }
 }
}
