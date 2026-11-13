import {createCanvas,loadImage} from '@napi-rs/canvas';
import {writeFile,mkdir} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {resolve} from 'node:path';
import {SpriteAtlas,PuppetRenderer} from '../dist/puppet.js';
import {AgentBrain} from '../dist/brain.js';
import {ROLES,OfficeModel,Camera,stationHome} from '../dist/office-model.js';
import {RoomRenderer} from '../dist/room.js';
const load=name=>loadImage(fileURLToPath(new URL('../dist/assets/'+name,import.meta.url)));
const [base,roles,props,floor]=await Promise.all(['architect-parts.png','roles.png','furniture.png','floor.png'].map(load));
const limbs=new SpriteAtlas(base,createCanvas),faces=new SpriteAtlas(roles,createCanvas),furniture=new SpriteAtlas(props,createCanvas,3,3,true),rigs={};
for(const [id,r] of Object.entries(ROLES))rigs[id]=new PuppetRenderer(limbs,faces.parts[r.index*2],faces.parts[r.index*2+1]);
const painter=new RoomRenderer({floor,furniture,rigs}),office=new OfficeModel(),brains=new Map(),camera=new Camera(),canvas=createCanvas(1280,800),c=canvas.getContext('2d');
const sync=()=>{for(const [i,a] of office.agents.entries())if(!brains.has(a.id))brains.set(a.id,new AgentBrain({...stationHome(office.deskFor(a).index),role:a.role,name:a.name,seed:i}));};
const output=resolve('qa-output');await mkdir(output,{recursive:true});
for(const count of [6,9,12]){
 while(office.agents.length<count)office.addAgent(Object.keys(ROLES)[office.agents.length%6]);sync();
 for(let i=0;i<90;i++)for(const b of brains.values())b.update(1/60);
 camera.fit(office.room,1280,800);painter.draw(c,office,brains,camera,{time:4,width:1280,height:800});
 await writeFile(`${output}/office-${count}.png`,canvas.toBuffer('image/png'));console.log('Rendered',count,'agents in',office.room);
}
