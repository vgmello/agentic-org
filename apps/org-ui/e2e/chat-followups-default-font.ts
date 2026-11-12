import { expect, test } from '@playwright/test';
import { shot } from './shot';
import { kindGeometry, navigation, openChat, sidebarGeometry } from './chat-followups';

for (const width of [320, 640, 960, 1280]) for (const scale of [1, 2]) {
	test(`BUG-94 BUG-70: sidebar preserves whole words at real20px default ${width}/${scale * 100}`, async ({ page }) => {
		await openChat(page, width, scale);
		await expect(page.locator('html')).toHaveCSS('font-size', `${20 * scale}px`);
		const nav = await navigation(page);
		await shot(page, `bug94-real20-${width}-${scale}`, { context: nav });
		await sidebarGeometry(nav);

	});
}

for (const width of [320,640,960,1280]) for (const scale of [1,2]) {
 if(width===960 && scale===2) continue; // Isolated BUG-119 below; all sidebar cases still run above.
 test(`BUG-95: real20px kind containment at ${width}/${scale*100}`,async({page})=>{
  await openChat(page,width,scale,'/chat?c=c-general');
  const badge=page.getByTestId('message').filter({hasText:'Ready for review: create-org'}).getByTitle('Announcement',{exact:true});
  await kindGeometry(badge);
  await shot(page,`bug95-real20-${width}-${scale}`,{context:badge.locator('..')});
 });
}

test('BUG-119: rem-scaled shell starves kind metadata at real20px 960/200', async ({page})=>{
 await openChat(page,960,2,'/chat?c=c-general');
 await expect(page.locator('html')).toHaveCSS('font-size','40px');
 const badge=page.getByTestId('message').filter({hasText:'Ready for review: create-org'}).getByTitle('Announcement',{exact:true});
 await expect(badge).toBeVisible();
 await shot(page,'bug119-shell-real20-960-200',{context:badge.locator('..')});
 test.info().annotations.push({type:'issue',description:'BUG-119: shell w-72/pl-72 consumes720/960px at40px root; Ada01M76V3DYH0BVT98EGJN5N2B7V carry. Remove expected failure when shell fix lands.'});
 test.fail(true,'BUG-119: unchanged rem-scaled shell; private MessageRow cannot fit265.55px in180px row');
 await kindGeometry(badge);
});
