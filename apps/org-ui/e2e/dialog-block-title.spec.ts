import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function openBoard(page: Page) {
 await visit(page,'/create');
 await page.getByLabel('Organization name').fill('Acme');
 await page.getByRole('button',{name:'Create organization',exact:true}).click();
 await expect(page).toHaveURL(/\/agents$/);
 await expect(page.getByRole('table',{name:'Agents',exact:true})).toBeVisible();
 await visit(page,'/projects/atlas');
 await page.getByRole('tab',{name:'Tasks',exact:true}).click();
}
async function blockNewTask(page: Page,name: string) {
 await page.getByRole('button',{name:'New task',exact:true}).click();
 const create=page.getByRole('dialog',{name:'New task',exact:true});
 await create.getByLabel('Title',{exact:true}).fill(name);
 await create.getByRole('button',{name:'Create task',exact:true}).click();
 await expect(create).toHaveCount(0);
 await page.getByTestId('task-card').filter({hasText:name}).getByRole('button',{name:/^Actions for/}).click();
 await page.getByRole('menuitem',{name:'Blocked',exact:true}).click();
 const dialog=page.locator('[data-slot=dialog-content]');await expect(dialog).toBeVisible();return dialog;
}
async function namedWhileClosing(page:Page,dialog:Locator,reference:string,via:'Escape'|'Cancel') {
 const expected=`Block ${reference}`;
 await expect(dialog).toHaveAccessibleName(expected);
 await dialog.evaluate(root=>{
  const sample=()=>({closed:root.getAttribute('data-state')==='closed'||root.hasAttribute('data-closed'),state:root.getAttribute('data-state'),label:root.getAttribute('aria-label'),ids:root.getAttribute('aria-labelledby'),headings:[...root.querySelectorAll('[data-slot=dialog-title]')].map(e=>({id:e.id,text:e.textContent}))});
  const samples=[sample()],observer=new MutationObserver(()=>{if(root.isConnected)samples.push(sample());});
  observer.observe(root,{subtree:true,childList:true,characterData:true,attributes:true,attributeFilter:['data-state','data-closed','aria-label','aria-labelledby']});
  Object.assign(window,{__blockTitleEvidence:{samples,observer}});
 });
 if(via==='Escape')await page.keyboard.press('Escape');else await dialog.getByRole('button',{name:'Cancel',exact:true}).click();
 await expect(dialog).toHaveCount(0);
 const samples=await page.evaluate(()=>{const w=window as unknown as {__blockTitleEvidence:{samples:{closed:boolean;label:string|null;ids:string|null;headings:{id:string;text:string|null}[]}[];observer:MutationObserver}};const evidence=w.__blockTitleEvidence;evidence.observer.disconnect();delete (w as Partial<typeof w>).__blockTitleEvidence;return evidence.samples;});
 await test.info().attach('block-closing-name',{body:JSON.stringify(samples),contentType:'application/json'});
 const closed=samples.filter(s=>s.closed);expect(closed.length,'actual framework closing state observed').toBeGreaterThan(0);
 for(const sample of closed){
  expect(sample.headings,'one Dialog title while closing').toHaveLength(1);
  expect(sample.headings[0].text,'closed heading keeps the task reference').toBe(expected);
  expect(sample.label,'no alternative aria-label supersedes the heading').toBeNull();
  expect(sample.ids?.split(/\s+/),'closed accessible name still references that heading').toContain(sample.headings[0].id);
 }
}

test('BUG-143/144: Block dialog name stays Block T-1 through the closing transition',async({page})=>{
 await openBoard(page);const dialog=await blockNewTask(page,'First title');await shot(page,'block-title-open-t1');
 await namedWhileClosing(page,dialog,'T-1','Escape');
});
test('BUG-143/144: Block title updates on each open and survives Cancel',async({page})=>{
 await openBoard(page);let dialog=await blockNewTask(page,'First title');
 await page.keyboard.press('Escape');await expect(dialog).toHaveCount(0);
 dialog=await blockNewTask(page,'Second title');await expect(dialog).toHaveAccessibleName('Block T-2');await shot(page,'block-title-open-t2');
 await namedWhileClosing(page,dialog,'T-2','Cancel');
});
