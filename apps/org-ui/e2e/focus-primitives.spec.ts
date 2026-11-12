import { expect, test } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { buttons, menu, consumerFocus, keyboardFocus, outline, token, setupFocus, readable, outlineContrast } from './focus-primitives';
for (const width of [320, 640, 960, 1280]) for (const scale of [1, 2]) {
	test(`BUG-101 BUG-70: menu keyboard and pointer contract at ${width}/${scale * 100}`, async ({ page }) => { await menu(page, width, scale); });
	test(`BUG-102 BUG-70: Button six variants and native actions at ${width}/${scale * 100}`, async ({ page }) => { await buttons(page, width, scale); });
	test(`BUG-101/102 BUG-70: nine consumer families at ${width}/${scale * 100}`, async ({ page }) => { await consumerFocus(page, width, scale); });
}

// WebKit has no exposed default-font preference in Playwright. This proves 40px-root rendering,
// while the dedicated Chromium project separately proves a real 20px browser default at 200%.
for (const width of [320, 640, 960, 1280]) {
	test(`BUG-101/102 BUG-70: explicit 40px root matrix at ${width}`, async ({ page }) => { await buttons(page, width, 2.5); });
	test(`BUG-101 BUG-70: explicit 40px root menu at ${width}`, async ({ page }) => { await menu(page, width, 2.5); });
	test(`BUG-102 BUG-70: explicit 40px root consumers at ${width}`, async ({ page }) => { await consumerFocus(page, width, 2.5); });
}

test('BUG-102 BUG-70: ProcessSheet caller override uses the same indigo focus and closes', async ({ page }) => {
	await visit(page, '/create'); await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click(); await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/processes?place=team:web'); await page.locator('[data-testid=process-row][data-key="code-review"]').click();
	const close = page.getByRole('dialog').getByRole('button', { name: 'Close', exact: true });
	await keyboardFocus(page, close); await outline(close, 2, await token(page));
	await shot(page, 'focus-processes-caller-override', { context: close });
	await close.press('Enter'); await expect(page.getByRole('dialog')).toHaveCount(0);
});

test('BUG-101/102 BUG-70: outline and readable-text guards allow real controls and catch collapsed or weak ones', async ({ page }) => {
	const fixture = await setupFocus(page, 320, 2);
	const control = fixture.getByRole('group', { name: 'default', exact: true }).getByRole('button');
	await expect(control).toHaveCount(1);
	await keyboardFocus(page, control); await outline(control, 2, await token(page)); await readable(control);
	const font = await control.evaluate((element) => getComputedStyle(element).fontSize);
	await control.evaluate((element) => { element.style.fontSize = '0px'; });
	await expect(control).toHaveCSS('font-size', '0px');
	await expect(readable(control)).rejects.toThrow();
	await control.evaluate((element) => { element.style.fontSize = ''; element.style.width = '1px'; element.style.padding = '0'; });
	await expect(control).toHaveCSS('font-size', font);
	await control.evaluate((element) => Promise.all(element.getAnimations().map((animation) => animation.finished.catch(() => undefined))));
	await expect(readable(control)).rejects.toThrow();
	await control.evaluate((element) => { element.style.width = ''; element.style.padding = ''; element.textContent = 'A'; });
	await control.evaluate((element) => Promise.all(element.getAnimations().map((animation) => animation.finished.catch(() => undefined))));
	await expect(readable(control)).rejects.toThrow();
	await control.evaluate((element) => { element.textContent = 'Activate'; element.style.outlineColor = 'var(--color-gray-600)'; });
	await expect(control).toHaveCSS('outline-color', await token(page, '--color-gray-600'));
	expect(await outlineContrast(control, false)).toBeLessThan(3);
	await control.evaluate((element) => { element.style.outlineColor = ''; });
	await outline(control, 2, await token(page)); await readable(control);
});

test('BUG-101 BUG-70: keyboard-to-pointer handoff keeps the pinned menu indication', async ({ page }) => {
	const fixture = await setupFocus(page, 320, 1);
	const trigger = fixture.getByRole('button', { name: 'Commands', exact: true });
	await keyboardFocus(page, trigger); await trigger.press('ArrowDown');
	await outline(page.getByRole('menuitem', { name: 'Rename item', exact: true }), -2, await token(page));
	const target = page.getByRole('menuitem', { name: 'Delete item', exact: true });
	await target.hover(); await expect(target).toBeFocused();
	console.info('Mixed-input native state:', await target.evaluate((element) => ({ visible: element.matches(':focus-visible'), style: getComputedStyle(element).outlineStyle, width: getComputedStyle(element).outlineWidth })));
	await shot(page, 'focus-keyboard-to-pointer', { context: page.getByRole('menu') });
	// Ada01M77BPB: preserve native focus-visible through hover and any subsequent pointer handoff.
	await outline(target, -2, await token(page));
	await target.click(); await expect(fixture.getByLabel('Selection')).toHaveText('Deleted');
	await expect(page.getByRole('menu')).toHaveCount(0);
	await trigger.click(); await target.hover(); await expect(target).toBeFocused();
	// Chromium retains the native indication here; WebKit clears it. The browser owns this state.
	const nativeVisible = await target.evaluate((element) => element.matches(':focus-visible'));
	console.info('Pointer reopen native focus-visible:', nativeVisible);
	if (nativeVisible) await outline(target, -2, await token(page));
	else await expect(target).toHaveCSS('outline-style', 'none');
	await shot(page, 'focus-pointer-reopen-native', { context: page.getByRole('menu') });
	await page.keyboard.press('Escape'); await expect(page.getByRole('menu')).toHaveCount(0);
	await fixture.getByTestId('focus-start').click(); await trigger.click(); await target.hover();
	await expect(target).toBeFocused();
	expect(await target.evaluate((element) => element.matches(':focus-visible'))).toBe(false);
	await expect(target).toHaveCSS('outline-style', 'none');
	await shot(page, 'focus-fresh-pointer-reset', { context: page.getByRole('menu') });
	await page.keyboard.press('Escape'); await expect(page.getByRole('menu')).toHaveCount(0);
});

for (const dismiss of ['blank', 'trigger', 'button'] as const) {
	test(`BUG-158 BUG-70: pointer ${dismiss} dismissal preserves the named focus destination`, async ({ page }) => {
		const fixture = await setupFocus(page, 640, 1);
		const trigger = fixture.getByRole('button', { name: 'Commands', exact: true });
		await trigger.click();
		await page.getByRole('menuitem', { name: 'Rename item', exact: true }).hover();
		const target = dismiss === 'button' ? fixture.getByRole('group', { name: 'default', exact: true }).getByRole('button') : trigger;
		const hit = await (dismiss === 'blank' ? fixture.locator('#focus-link') : target).boundingBox();
		expect(hit).not.toBeNull();
		await page.mouse.click(hit!.x + hit!.width / 2, hit!.y + hit!.height / 2);
		await expect(page.getByRole('menu')).toHaveCount(0);
		await shot(page, `bug158-${dismiss}-dismissal`);
		if (dismiss === 'button') await expect(fixture.getByLabel('Activations')).toHaveText('1');
		await expect(target).toBeFocused();
		await expect.poll(() => target.evaluate((node) => node.matches(':focus-visible'))).toBe(false);
	});
}

async function pointerFixture(page: import('@playwright/test').Page, mode = 'normal', width = 640, scale = 1) {
	await setupFocus(page, width, scale);
	await page.evaluate(async (mode) => {
		const path = '/src/lib/components/testing/focus-primitives.ts';
		const { mountPointerMenuContracts } = await import(path);
		const target = document.createElement('div'); document.body.append(target); mountPointerMenuContracts(target, mode);
	}, mode);
	return page.getByRole('region', { name: 'Pointer menu contracts', exact: true });
}
async function physicalClick(page: import('@playwright/test').Page, target: import('@playwright/test').Locator) {
	const box = await target.boundingBox(); expect(box).not.toBeNull();
	await page.mouse.click(box!.x + box!.width / 2, box!.y + box!.height / 2);
}
for (const destination of ['Outside input', 'Outside action', 'Other menu', 'Outside dialog'] as const) {
	test(`BUG-158 BUG-70: outside ${destination} owns focus and activates once`, async ({ page }) => {
		const fixture = await pointerFixture(page);
		await fixture.getByRole('button', { name: 'Contract commands', exact: true }).click();
		await page.getByRole('menuitem', { name: 'Act in place', exact: true }).hover();
		const target = destination === 'Outside input' ? fixture.getByRole('textbox', { name: destination }) : fixture.getByRole('button', { name: destination, exact: true });
		await physicalClick(page, destination === 'Outside action' ? target.locator('span') : target);
		await expect(page.getByRole('menuitem', { name: 'Act in place', exact: true })).toHaveCount(0);
		if (destination === 'Other menu') {
			await expect(page.getByRole('menuitem', { name: 'Other action', exact: true })).toBeVisible();
			await expect.poll(() => page.getByRole('menu').evaluate((menu) => menu.contains(document.activeElement))).toBe(true);
		} else if (destination === 'Outside dialog') {
			await expect(page.getByRole('dialog')).toBeVisible();
			await expect.poll(() => page.getByRole('dialog').evaluate((dialog) => dialog.contains(document.activeElement))).toBe(true);
		} else {
			await expect(target).toBeFocused();
			if (destination === 'Outside action') {
				await expect(fixture.getByLabel('Pointer activations')).toHaveText('1');
				await expect.poll(() => target.evaluate((node) => node.matches(':focus-visible'))).toBe(false);
			}
		}
		await shot(page, `bug158-outside-${destination.replaceAll(' ', '-')}`);
	});
}
for (const mode of ['prevent-outside', 'prevent-close', 'caller-focus'] as const) {
	test(`BUG-158 BUG-70: ${mode} caller policy wins over restoration`, async ({ page }) => {
		const fixture = await pointerFixture(page, mode);
		await fixture.getByRole('button', { name: 'Contract commands', exact: true }).click();
		await page.getByRole('menuitem', { name: 'Act in place', exact: true }).hover();
		await physicalClick(page, fixture.getByTestId('blank'));
		if (mode === 'prevent-outside') await expect(page.getByRole('menu')).toBeVisible();
		else {
			await expect(page.getByRole('menu')).toHaveCount(0);
			await expect(fixture.getByRole('button', { name: 'Caller destination', exact: true })).toBeFocused();
		}
		await shot(page, `bug158-${mode}`);
	});
}
for (const mode of ['remove-trigger', 'disable-trigger'] as const) {
	test(`BUG-158 BUG-70: ${mode} never invents a caller fallback`, async ({ page }) => {
		const fixture = await pointerFixture(page, mode);
		const trigger = fixture.getByRole('button', { name: 'Contract commands', exact: true });
		await trigger.click(); await page.getByRole('menuitem', { name: 'Act in place', exact: true }).hover();
		await physicalClick(page, fixture.getByTestId('blank'));
		await expect(page.getByRole('menu')).toHaveCount(0);
		if (mode === 'remove-trigger') await expect(trigger).toHaveCount(0); else await expect(trigger).toBeDisabled();
		await expect(fixture.getByRole('button', { name: 'Caller destination', exact: true })).not.toBeFocused();
		await shot(page, `bug158-${mode}-caller-gap`);
	});
}

test('BUG-158 BUG-70: keyboard, item actions, prevented selection and dialog destination retain their policies', async ({ page }) => {
	const fixture = await pointerFixture(page), trigger = fixture.getByRole('button', { name: 'Contract commands', exact: true });
	await keyboardFocus(page, trigger); await trigger.press('ArrowDown');
	const first = page.getByRole('menuitem', { name: 'Act in place', exact: true }); await expect(first).toBeFocused();
	await first.press('Enter'); await expect(fixture.getByLabel('Pointer activations')).toHaveText('1'); await expect(trigger).toBeFocused();
	await trigger.press('ArrowDown'); await page.getByRole('menuitem', { name: 'Keep open', exact: true }).click();
	await expect(page.getByRole('menu')).toBeVisible();
	await page.getByRole('menuitemcheckbox', { name: 'Checked option', exact: true }).click();
	await expect(fixture.getByLabel('Checkbox value')).toHaveText('true'); await expect(page.getByRole('menu')).toBeVisible();
	await page.getByRole('menuitemradio', { name: 'Choice two', exact: true }).click();
	await expect(fixture.getByLabel('Radio value')).toHaveText('two'); await expect(page.getByRole('menu')).toBeVisible();
	await page.getByRole('menuitemradio', { name: 'Closing choice', exact: true }).click();
	await expect(fixture.getByLabel('Radio value')).toHaveText('close'); await expect(page.getByRole('menu')).toHaveCount(0); await expect(trigger).toBeFocused();
	await trigger.press('ArrowDown'); await expect(first).toBeFocused(); await page.keyboard.press('Escape'); await expect(trigger).toBeFocused();
	await trigger.press('ArrowDown'); await expect(first).toBeFocused(); await page.keyboard.press(page.context().browser()?.browserType().name() === 'webkit' && process.platform === 'darwin' ? 'Alt+Tab' : 'Tab');
	await expect(page.getByRole('menu')).toHaveCount(0); await expect(fixture.getByRole('button', { name: 'Other menu', exact: true })).toBeFocused();
	await trigger.click(); await page.getByRole('menuitem', { name: 'Open dialog', exact: true }).click();
	await expect(page.getByRole('dialog')).toBeVisible(); await expect(page.getByRole('textbox', { name: 'Dialog field', exact: true })).toBeFocused();
	await shot(page, 'bug158-item-dialog-focus'); await page.getByRole('button', { name: 'Close destination', exact: true }).click();
	await expect(page.getByRole('dialog')).toHaveCount(0); await expect(trigger).toBeFocused();
});

test('BUG-158 BUG-70: wheel scrolling keeps the open menu anchored and focus within it', async ({ page }) => {
	const fixture = await pointerFixture(page, 'normal', 1280);
	const trigger = fixture.getByRole('button', { name: 'Contract commands', exact: true });
	await trigger.click(); await page.getByRole('menuitem', { name: 'Act in place', exact: true }).hover();
	const menu = page.getByRole('menu');
	const gap = async () => { const a = await menu.boundingBox(), b = await trigger.boundingBox(); expect(a).not.toBeNull(); expect(b).not.toBeNull(); return a!.y - b!.y - b!.height; };
	const before = await gap();
	await page.mouse.move(1100, 800); await page.mouse.wheel(0, 20);
	await expect.poll(() => fixture.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
	await expect(menu).toBeVisible(); await expect.poll(gap).toBeCloseTo(before, 0);
	await expect.poll(() => menu.evaluate((node) => node.contains(document.activeElement))).toBe(true);
	console.info('BUG158 wheel: menu repositions, remains open, focus stays in menu');
	await shot(page, 'bug158-wheel-anchored');
});

for (const mode of ['normal', 'prevent-outside'] as const) {
 test(`BUG-158 BUG-70: dispatch outside remains outside when a control reflows the menu (${mode})`, async ({ page }) => {
  const fixture = await pointerFixture(page, mode);
  await fixture.getByRole('button', { name: 'Contract commands', exact: true }).click();
  const menu = page.getByRole('menu'); await expect(menu).toBeVisible();
  await shot(page, `bug158-before-reflow-${mode}`);
  const target = fixture.getByRole('button', { name: 'Outside action', exact: true });
  const menuId = await menu.getAttribute('id'); expect(menuId).toBeTruthy();
  await target.evaluate((node, menuId) => node.addEventListener('click', event => {
   const menu = document.getElementById(menuId!)!, rect = menu.getBoundingClientRect();
   node.dataset.outsideAtDispatch = String(event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom);
   menu.style.setProperty('transition', 'none', 'important');
   menu.style.setProperty('margin-left', `${event.clientX - rect.left - 20}px`, 'important');
   menu.style.setProperty('margin-top', `${event.clientY - rect.top - 20}px`, 'important');
   const moved = menu.getBoundingClientRect();
   node.dataset.insideAfterReflow = String(event.clientX > moved.left && event.clientX < moved.right && event.clientY > moved.top && event.clientY < moved.bottom);
   node.dataset.geometry = JSON.stringify({ point: [event.clientX,event.clientY], before: rect.toJSON(), after: moved.toJSON(), style: menu.style.cssText, computed: {margin: getComputedStyle(menu).marginLeft, translate: getComputedStyle(menu).translate, display:getComputedStyle(menu).display, transition:getComputedStyle(menu).transition}, duplicates: document.querySelectorAll(`[id="${menu.id}"]`).length, parent: menu.parentElement?.outerHTML.slice(0,500) });
  }, { once: true }), menuId);
  await physicalClick(page, target);
  await expect(target).toHaveAttribute('data-outside-at-dispatch', 'true'); expect(await target.getAttribute('data-inside-after-reflow'), await target.getAttribute('data-geometry') ?? '').toBe('true');
  await expect(fixture.getByLabel('Pointer activations')).toHaveText('1');
  if (mode === 'normal') await expect(menu).toHaveCount(0); else await expect(menu).toBeVisible();
  await shot(page, `bug158-after-reflow-${mode}`);
 });
}

for (const mode of ['normal', 'caller-valid'] as const) {
 test(`BUG-158 BUG-70: right click respects default validity and caller override (${mode})`, async ({ page }) => {
  const fixture=await pointerFixture(page,mode); await fixture.getByRole('button',{name:'Contract commands',exact:true}).click();
  await shot(page,`bug158-right-before-${mode}`);
  const box=(await fixture.getByTestId('blank').boundingBox())!; await page.mouse.click(box.x+box.width/2,box.y+box.height/2,{button:'right'});
  if(mode==='normal') await expect(page.getByRole('menu')).toBeVisible(); else await expect(page.getByRole('menu')).toHaveCount(0);
  await shot(page,`bug158-right-after-${mode}`);
 });
}
for (const state of ['disabled','inert'] as const) {
 test(`BUG-158 BUG-70: an outside control becoming ${state} returns to the trigger`, async ({page}) => {
  const fixture=await pointerFixture(page), trigger=fixture.getByRole('button',{name:'Contract commands',exact:true});
  const target=fixture.getByRole('button',{name:'Outside action',exact:true});
  await target.evaluate((node,state)=>node.addEventListener('click',()=>{node.dataset.transitions=String(Number(node.dataset.transitions??0)+1);node.setAttribute(state,'');},{once:true}),state);
  await trigger.click(); await page.getByRole('menuitem',{name:'Act in place',exact:true}).hover(); await physicalClick(page,target);
  await expect(target).toHaveAttribute('data-transitions','1'); await expect(target).toHaveAttribute(state,''); await expect(page.getByRole('menu')).toHaveCount(0); await expect(trigger).toBeFocused();
  await shot(page,`bug158-outside-becomes-${state}`);
 });
}
test('BUG-158 BUG-70: a shadow button retains its dispatch path and native focus',async({page})=>{
 const fixture=await pointerFixture(page);
 await fixture.evaluate(node=>{const host=document.createElement('div');host.id='shadow-outside';host.style.cssText='position:fixed;right:20px;bottom:20px';host.attachShadow({mode:'open'}).innerHTML='<button>Shadow action</button>';node.append(host);});
 const target=page.locator('#shadow-outside').getByRole('button',{name:'Shadow action',exact:true});
 await fixture.getByRole('button',{name:'Contract commands',exact:true}).click(); await page.getByRole('menuitem',{name:'Act in place',exact:true}).hover();
 await physicalClick(page,target); await expect(page.getByRole('menu')).toHaveCount(0); await expect(target).toBeFocused(); await shot(page,'bug158-shadow-button');
});
test('BUG-158 BUG-70: submenu parent and whole-menu focus policies survive',async({page})=>{
 const fixture=await pointerFixture(page),trigger=fixture.getByRole('button',{name:'Contract commands',exact:true});
 await keyboardFocus(page,trigger); await trigger.press('ArrowDown');
 const parent=page.getByRole('menuitem',{name:'More commands',exact:true}); await parent.focus(); await parent.press('ArrowRight');
 const nested=page.getByRole('menuitem',{name:'Nested action',exact:true}); await expect(nested).toBeFocused();
 await nested.press('ArrowLeft'); await expect(parent).toBeFocused(); await expect(page.getByRole('menu')).toHaveCount(1);
 await parent.press('ArrowRight'); await expect(nested).toBeFocused(); await nested.click();
 await expect(fixture.getByLabel('Pointer activations')).toHaveText('1'); await expect(page.getByRole('menu')).toHaveCount(0); await expect(trigger).toBeFocused();
 await trigger.click(); await parent.hover(); await expect(nested).toBeVisible();
 await nested.hover(); await expect(page.getByRole('menu')).toHaveCount(2); await expect(parent).toBeVisible();
 await shot(page,'bug158-submenu-inside'); await physicalClick(page,fixture.getByTestId('blank'));
 await expect(page.getByRole('menu')).toHaveCount(0); await expect(trigger).toBeFocused(); await shot(page,'bug158-submenu-outside');
});
test('BUG-158 BUG-70: scoped dispatch capture is removed when content unmounts',async({page})=>{
 const fixture=await pointerFixture(page);
 await page.evaluate(()=>{
  const add=document.addEventListener.bind(document),remove=document.removeEventListener.bind(document),listeners=new Map<EventListener,Map<string,EventListener>>();
  const state={calls:0,active:0}; (window as unknown as {__menuCapture:typeof state}).__menuCapture=state;
  document.addEventListener=((type:string,listener:EventListener,options?:boolean|AddEventListenerOptions)=>{
   if(listener?.name==='captureMenuDispatch'){ const wrapped:EventListener=e=>{state.calls++;listener(e);};const types=listeners.get(listener)??new Map<string,EventListener>();types.set(type,wrapped);listeners.set(listener,types);state.active++;add(type,wrapped,options); } else add(type,listener,options);
  }) as typeof document.addEventListener;
  document.removeEventListener=((type:string,listener:EventListener,options?:boolean|EventListenerOptions)=>{
   const wrapped=listeners.get(listener)?.get(type);if(wrapped){state.active--;remove(type,wrapped,options);}else remove(type,listener,options);
  }) as typeof document.removeEventListener;
 });
 await fixture.getByRole('button',{name:'Contract commands',exact:true}).click(); await expect(page.getByRole('menu')).toBeVisible();
 const state=()=>page.evaluate(()=>(window as unknown as {__menuCapture:{calls:number;active:number}}).__menuCapture);
 await expect.poll(async()=>(await state()).active).toBe(2);
 await physicalClick(page,fixture.getByTestId('blank')); await expect(page.getByRole('menu')).toHaveCount(0); await expect.poll(async()=>(await state()).active).toBe(0);
 const calls=(await state()).calls; expect(calls).toBeGreaterThan(0); await physicalClick(page,fixture.getByTestId('blank')); expect((await state()).calls).toBe(calls);
 await shot(page,'bug158-recorder-cleaned');
});

test('BUG-158 BUG-70: a caller can prevent restoration without choosing a replacement',async({page})=>{
 const fixture=await pointerFixture(page,'prevent-close-only'),trigger=fixture.getByRole('button',{name:'Contract commands',exact:true});
 await trigger.click(); await page.getByRole('menuitem',{name:'Act in place',exact:true}).hover(); await physicalClick(page,fixture.getByTestId('blank'));
 await expect(page.getByRole('menu')).toHaveCount(0); await expect(page.locator('body')).toBeFocused(); await shot(page,'bug158-prevent-close-only');
});

test('BUG-158 BUG-70: a non-element event target retains Bits validity exclusion',async({page})=>{
 const fixture=await pointerFixture(page); await fixture.getByRole('button',{name:'Contract commands',exact:true}).click();
 const menu=page.getByRole('menu'); await shot(page,'bug158-non-element-before');
 await fixture.getByTestId('blank').evaluate(node=>{const r=node.getBoundingClientRect();node.firstChild!.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,composed:true,button:0,pointerType:'mouse',clientX:r.x+r.width/2,clientY:r.y+r.height/2}));});
 await page.evaluate(()=>new Promise<void>(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>resolve()))));
 await shot(page,'bug158-non-element-settled'); await expect(menu).toBeVisible();
});

test('BUG-158 BUG-70: an explicit scroll-lock caller keeps the opt-in policy',async({page})=>{
 const fixture=await pointerFixture(page,'scroll-lock'),trigger=fixture.getByRole('button',{name:'Contract commands',exact:true});
 await trigger.click(); await page.getByRole('menuitem',{name:'Act in place',exact:true}).hover();
 await expect(page.locator('body')).toHaveCSS('pointer-events','none');
 await physicalClick(page,fixture.getByRole('button',{name:'Outside action',exact:true}));
 await expect(page.getByRole('menu')).toHaveCount(0); await expect(fixture.getByLabel('Pointer activations')).toHaveText('0');
 await expect(page.locator('body')).not.toHaveCSS('pointer-events','none'); await shot(page,'bug158-explicit-scroll-lock');
});
test.describe('BUG-158 touch contract',()=>{
 test.use({hasTouch:true});
 test('BUG-158 BUG-70: touch activation preserves the undebounced outside path',async({page})=>{
  const fixture=await pointerFixture(page),trigger=fixture.getByRole('button',{name:'Contract commands',exact:true}),target=fixture.getByRole('button',{name:'Outside action',exact:true});
  await trigger.tap(); await expect(page.getByRole('menu')).toBeVisible();
  const box=(await target.boundingBox())!;await page.touchscreen.tap(box.x+box.width/2,box.y+box.height/2);
  await expect(fixture.getByLabel('Pointer activations')).toHaveText('1'); await expect(page.getByRole('menu')).toHaveCount(0); await expect(target).toBeFocused(); await shot(page,'bug158-touch-outside');
 });
});

test('BUG-158 BUG-70: prevented outside interaction does not leak a pointer destination into keyboard selection',async({page})=>{
 const fixture=await pointerFixture(page,'prevent-outside'),trigger=fixture.getByRole('button',{name:'Contract commands',exact:true});
 await trigger.click();await page.getByRole('menuitem',{name:'Act in place',exact:true}).hover();await physicalClick(page,fixture.getByRole('button',{name:'Outside action',exact:true}));
 await expect(fixture.getByLabel('Pointer activations')).toHaveText('1');await expect(page.getByRole('menu')).toBeVisible();
 const action=page.getByRole('menuitem',{name:'Act in place',exact:true});await action.focus();await action.press('Enter');await expect(page.getByRole('menu')).toHaveCount(0);await expect(trigger).toBeFocused();await shot(page,'bug158-prevented-then-keyboard');
});
test('BUG-158 BUG-70: a pointer close plan is consumed before a later keyboard close',async({page})=>{
 const fixture=await pointerFixture(page),trigger=fixture.getByRole('button',{name:'Contract commands',exact:true});
 await trigger.click();await page.getByRole('menuitem',{name:'Act in place',exact:true}).hover();await physicalClick(page,fixture.getByRole('button',{name:'Outside action',exact:true}));
 await expect(page.getByRole('menu')).toHaveCount(0);await expect(fixture.getByRole('button',{name:'Outside action',exact:true})).toBeFocused();
 await keyboardFocus(page,trigger);await trigger.press('ArrowDown');const action=page.getByRole('menuitem',{name:'Act in place',exact:true});await expect(action).toBeFocused();await action.press('Enter');await expect(page.getByRole('menu')).toHaveCount(0);await expect(trigger).toBeFocused();await shot(page,'bug158-consumed-plan');
});
test('BUG-158 BUG-70: caller removal during outside interaction safely relinquishes content ownership',async({page})=>{
 const errors:string[]=[];page.on('pageerror',error=>errors.push(error.message));const fixture=await pointerFixture(page,'remove-content');
 await fixture.getByRole('button',{name:'Contract commands',exact:true}).click();await page.getByRole('menuitem',{name:'Act in place',exact:true}).hover();await physicalClick(page,fixture.getByTestId('blank'));
 await expect(page.getByRole('menu')).toHaveCount(0);await expect(fixture.getByRole('button',{name:'Contract commands',exact:true})).toHaveCount(0);await shot(page,'bug158-caller-removed-content');expect(errors).toEqual([]);
});
test('BUG-158 BUG-70: an immediate caller reopen never lends focus back to the trigger',async({page})=>{
 const fixture=await pointerFixture(page,'reopen'),trigger=fixture.getByRole('button',{name:'Contract commands',exact:true});
 await trigger.click();await page.getByRole('menuitem',{name:'Act in place',exact:true}).hover();
 await trigger.evaluate(node=>{node.dataset.focuses='0';node.addEventListener('focus',()=>node.dataset.focuses=String(Number(node.dataset.focuses)+1));});
 await physicalClick(page,fixture.getByTestId('blank'));await shot(page,'bug158-immediate-reopen');
 await expect(page.getByRole('menu')).toBeVisible();await expect.poll(()=>page.getByRole('menu').evaluate(node=>node.contains(document.activeElement))).toBe(true);await expect(trigger).toHaveAttribute('data-focuses','0');
});
