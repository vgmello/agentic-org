import test from 'node:test';
import assert from 'node:assert/strict';
import {AmbientClock,cubic} from '../dist/activity.js';

test('pause, hidden tabs, and long stalls do not jump the simulation forward',()=>{
  const clock=new AmbientClock();clock.tick(1000);clock.tick(1050);
  clock.suspend();clock.tick(9000000);assert.equal(clock.elapsed,.05);
  clock.tick(9000050);assert.equal(clock.elapsed,.1);
  clock.tick(9999999);assert.equal(clock.elapsed,.25);
});
test('light trajectories meet exactly at curve endpoints',()=>{
  assert.deepEqual(cubic([1,2],[3,4],[5,6],[7,8],0),{x:1,y:2});
  assert.deepEqual(cubic([1,2],[3,4],[5,6],[7,8],1),{x:7,y:8});
});
