import test from 'node:test';
import assert from 'node:assert/strict';
import {AgentBrain,agentHitBox,pointInAgent} from '../dist/brain.js';
import {SpriteAtlas,PuppetRenderer} from '../dist/puppet.js';
import {createCanvas,loadImage} from '@napi-rs/canvas';
import {fileURLToPath} from 'node:url';
const advance=(brain,seconds)=>{for(let i=0;i<seconds*60;i++)brain.update(1/60)};

test('autonomous routine has visible movement, gestures, and completed work',()=>{
  const brain=new AgentBrain(),seen=new Set();let minX=Infinity,maxX=-Infinity,minArm=Infinity,maxArm=-Infinity;
  for(let i=0;i<65*60;i++){
    brain.update(1/60);seen.add(brain.state.kind);minX=Math.min(minX,brain.x);maxX=Math.max(maxX,brain.x);minArm=Math.min(minArm,brain.pose.rightArm);maxArm=Math.max(maxArm,brain.pose.rightArm);
    assert.ok(brain.x>=230&&brain.x<=280&&brain.y>=582&&brain.y<=606,'Agent left the walkable space');
    for(const value of Object.values(brain.pose))assert.ok(Number.isFinite(value));
  }
  assert.deepEqual(seen,new Set(['hello','work','walk','think','inspect','stretch']));
  assert.ok(maxX-minX>=40,'Autonomous steps must be visible');
  assert.ok(maxArm-minArm>1.5,'Greeting must clearly lift the arm');
  assert.ok(brain.completed>=3);
});
test('click reactions interrupt smoothly and return to independent activity',()=>{
  const brain=new AgentBrain();advance(brain,10.5);assert.equal(brain.state.kind,'walk');
  const before={x:brain.x,y:brain.y,arm:brain.pose.rightArm};
  assert.equal(brain.request('hello'),true);
  assert.equal(brain.x,before.x);assert.equal(brain.y,before.y);assert.equal(brain.pose.rightArm,before.arm);
  advance(brain,1);assert.ok(brain.pose.rightArm<-1.8);assert.match(brain.bubble,/Hello/);
  advance(brain,4);assert.notEqual(brain.state.kind,'hello');
  brain.request('blueprint');advance(brain,1);assert.ok(brain.pose.leftArm>1);assert.ok(brain.pose.hologram>.2);
  brain.request('break');advance(brain,3.1);assert.equal(brain.state.kind,'stretch');
  advance(brain,4);assert.notEqual(brain.state.kind,'stretch');
  assert.equal(brain.request('invalid'),false);
});
test('the interaction bounds follow the moving character and include its head and body',()=>{
  const brain=new AgentBrain(),before=agentHitBox(brain);advance(brain,12.5);const after=agentHitBox(brain);
  assert.ok(after.x>before.x+20);
  for(const dy of [-175,-120,-40])assert.equal(pointInAgent(brain,brain.x,brain.y+dy),true);
  assert.equal(pointInAgent(brain,650,800),false);
  // A tap on a half-size mobile scene maps back to the same world point.
  const viewportWidth=384,origin=20,clientX=origin+brain.x*viewportWidth/768;
  assert.equal(pointInAgent(brain,(clientX-origin)/viewportWidth*768,brain.y-140),true);
});
test('actual sprite atlas renders all 12 parts, with different pixels for different poses',async()=>{
  const image=await loadImage(fileURLToPath(new URL('../dist/assets/architect-parts.png',import.meta.url)));
  const atlas=new SpriteAtlas(image,createCanvas);assert.equal(atlas.parts.length,12);
  for(const part of atlas.parts)assert.ok(part.width>30&&part.height>30);
  const rig=new PuppetRenderer(atlas),brain=new AgentBrain(),canvas=createCanvas(768,1024),ctx=canvas.getContext('2d');
  advance(brain,.8);rig.draw(ctx,brain);const first=ctx.getImageData(0,0,768,1024).data;
  const beforePosition=brain.x;
  advance(brain,11.7);ctx.clearRect(0,0,768,1024);rig.draw(ctx,brain);const next=ctx.getImageData(0,0,768,1024).data;
  let changed=0,opaque=0,revealed=0;
  for(let i=3;i<first.length;i+=4){if(Math.abs(first[i]-next[i])>50)changed++;if(first[i]>200)opaque++;if(first[i]>200&&next[i]<10)revealed++;}
  assert.ok(opaque>5000,'Character should have substantial opaque art');
  assert.ok(changed>5000,'Different poses should visibly move the sprite');
  assert.ok(revealed>3000,'Movement should actually reveal background, not distort it');
  assert.ok(brain.x-beforePosition>20);
});
