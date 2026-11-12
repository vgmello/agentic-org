import { expect, test } from '@playwright/test';
import { openTitle, titles, titleFlow, titleText, closeFootprintFixture, wordGeometry, settle, legacyColumnControl, type TitleCase } from './dialog-title-open';
import { shot } from './shot';

for(const kind of Object.keys(titles) as TitleCase[])for(const width of [320,640,960,1280])for(const scale of [1,2,2.5]) {
 test(`BUG-143/144 B: ${kind} title text and words ${width}/root${16*scale}`,async({page})=>{
  try{await titleFlow(page,kind,width,scale);}finally{await shot(page,`title-b-${kind}-${width}-${16*scale}`);}
 });
}

for(const kind of ['remove-project','move-team','assign'] as const) {
 test(`BUG-143/144 B: ${kind} atomic words beside future A float at320/root40`,async({page})=>{
  const dialog=await openTitle(page,kind,320,2.5);
  await closeFootprintFixture(page);await settle(page);
  expect(await dialog.locator('[data-slot=dialog-title]').evaluate(e=>e.getBoundingClientRect().width),'future A fixture title column').toBe(168);
  try{await titleText(page,dialog,titles[kind]);await wordGeometry(dialog);}finally{await shot(page,`title-b-${kind}-word-composition-320-root40`);}
 });
}

for(const name of ['W'.repeat(40),'Unassigned','Remove Atlas from Platform','<img src=x onerror=alert(1)>']) {
 test(`BUG-143/144 B: runtime team name ${name} stays literal and atomic`,async({page})=>{
  let dialog=await openTitle(page,'rename-team',320);
  await dialog.getByLabel('Name',{exact:true}).fill(name);
  await dialog.getByRole('button',{name:'Rename',exact:true}).click();
  await expect(dialog).toHaveCount(0);
  await page.getByRole('button',{name:`Actions for ${name}`,exact:true}).click();
  await page.getByRole('menuitem',{name:'Rename…',exact:true}).click();
  dialog=page.locator('[data-slot=dialog-content]');await expect(dialog).toBeVisible();
  await page.locator('html').evaluate(e=>e.style.fontSize='250%');
  await expect(page.locator('html')).toHaveCSS('font-size','40px');await settle(page);
  await titleText(page,dialog,`Rename ${name}`);await legacyColumnControl(page,dialog);await wordGeometry(dialog);
  await closeFootprintFixture(page);await settle(page);
  try{await titleText(page,dialog,`Rename ${name}`);await legacyColumnControl(page,dialog);await wordGeometry(dialog);}finally{await shot(page,`title-b-runtime-${name.length}-320-root40`);}
 });
}

// Live office updates must update the same mounted heading, including branch changes.
test('BUG-143/144 B: live refusal updates mounted title without stale word spans',async({page})=>{
 const dialog=await openTitle(page,'delete',640);
 const title=dialog.locator('[data-slot=dialog-title]'),id=await title.getAttribute('id');
 await titleText(page,dialog,'Delete People?');
 const organizationId=await page.evaluate(()=>{const saved=JSON.parse(localStorage.getItem('agentic-org.mock.connections')??'[]');return saved[saved.length-1].organization.id as string;});
 await page.evaluate(id=>(window as unknown as {__mockOrgChange:(id:string,command:object)=>Promise<unknown>}).__mockOrgChange(id,{method:'team.create',params:{team_id:'title-child',name:'Child team',owner_team_id:'people'}}),organizationId);
 await expect(title).toHaveText("Can't delete People");await titleText(page,dialog,"Can't delete People");
 await expect(title).toHaveAttribute('id',id!);await wordGeometry(dialog);await shot(page,'title-b-live-refusal');
 await page.evaluate(id=>(window as unknown as {__mockOrgChange:(id:string,command:object)=>Promise<unknown>}).__mockOrgChange(id,{method:'team.delete',params:{team_id:'title-child',expected_version:1}}),organizationId);
 await expect(title).toHaveText('Delete People?');await titleText(page,dialog,'Delete People?');
 await expect(title).toHaveAttribute('id',id!);await expect(dialog.locator('[data-slot=dialog-title]')).toHaveCount(1);await shot(page,'title-b-live-restored');
});
