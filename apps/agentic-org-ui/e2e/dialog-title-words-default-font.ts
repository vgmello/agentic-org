import { test } from '@playwright/test';
import { titles, titleFlow, type TitleCase } from './dialog-title-open';
import { shot } from './shot';
for(const kind of Object.keys(titles) as TitleCase[])for(const width of [320,640,960,1280])for(const scale of [1,2]) {
 test(`BUG-143/144 B: ${kind} real20 default title ${width}/${scale*100}`,async({page})=>{
  try{await titleFlow(page,kind,width,scale,20);}finally{await shot(page,`title-b-real20-${kind}-${width}-${scale}`);}
 });
}
