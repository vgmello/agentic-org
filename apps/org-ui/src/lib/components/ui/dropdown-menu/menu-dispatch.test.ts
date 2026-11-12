import { expect, it } from 'vitest';
import { isOutsideAtDispatch } from './menu-dispatch.svelte';
const node = {} as HTMLElement;
const snapshot = { node, path: [], target: { isConnected: true } as Element, button: 0, pointerType: 'mouse', x: 9, y: 15, rect: { left: 10, right: 20, top: 10, bottom: 20 } as DOMRect, insideTree: false };
it('has no widening without the exact-event snapshot', () => expect(isOutsideAtDispatch(undefined, node)).toBe(false));
it('does not reuse a snapshot from another content', () => expect(isOutsideAtDispatch(snapshot, {} as HTMLElement)).toBe(false));
for (const [name, patch] of [
 ['non-element target', {target: null}], ['right button', {button: 2}], ['touch', {pointerType: 'touch'}],
 ['disconnected target', {target: {isConnected: false} as Element}], ['same or uncertain menu tree', {insideTree: true}]
] as const) it(`preserves Bits exclusion for ${name}`, () => expect(isOutsideAtDispatch({...snapshot, ...patch}, node)).toBe(false));
for (const [name, x, y, expected] of [
 ['left outside',9,15,true],['left boundary',10,15,false],['left inside',11,15,false],
 ['right inside',19,15,false],['right boundary',20,15,false],['right outside',21,15,true],
 ['top outside',15,9,true],['top boundary',15,10,false],['top inside',15,11,false],
 ['bottom inside',15,19,false],['bottom boundary',15,20,false],['bottom outside',15,21,true]
] as const) it(`uses dispatch geometry: ${name}`, () => expect(isOutsideAtDispatch({...snapshot,x,y}, node)).toBe(expected));
it('allows a non-touch pen primary pointer', () => expect(isOutsideAtDispatch({...snapshot,pointerType:'pen'},node)).toBe(true));

import { afterAll, beforeAll, vi } from 'vitest';
import { resolveMenuRoot } from './menu-dispatch.svelte';
beforeAll(() => vi.stubGlobal('CSS', { escape: (id: string) => id }));
afterAll(() => vi.unstubAllGlobals());
function menuGraph() {
 const triggers = new Map<string, {isConnected: boolean; closest: () => HTMLElement | null}>();
 const ownerDocument = { querySelector: (selector: string) => triggers.get(selector.match(/~="(.*)"/)![1]) ?? null };
 const menu = (id: string, parent: HTMLElement | null = null) => {
  const node = {id, isConnected:true, ownerDocument} as unknown as HTMLElement;
  triggers.set(id, {isConnected:true, closest: () => parent}); return node;
 };
 return {menu,triggers};
}
it('resolves nested portals to the same root menu', () => {
 const g=menuGraph(), root=g.menu('root'), sub=g.menu('sub',root), nested=g.menu('nested',sub);
 expect(resolveMenuRoot(root)).toBe(root); expect(resolveMenuRoot(sub)).toBe(root); expect(resolveMenuRoot(nested)).toBe(root);
 expect(resolveMenuRoot(g.menu('other'))).not.toBe(root);
});
it('fails conservatively for a missing controlling trigger', () => {
 const g=menuGraph(), root=g.menu('root'); g.triggers.delete('root'); expect(resolveMenuRoot(root)).toBeNull();
});
it('fails conservatively for a detached controlling trigger', () => {
 const g=menuGraph(), root=g.menu('root'); g.triggers.get('root')!.isConnected=false; expect(resolveMenuRoot(root)).toBeNull();
});
it('fails conservatively for a detached parent menu', () => {
 const g=menuGraph(), root=g.menu('root'), sub=g.menu('sub',root); Object.assign(root,{isConnected:false}); expect(resolveMenuRoot(sub)).toBeNull();
});
it('fails conservatively for a cyclic control relationship', () => {
 const g=menuGraph(), root=g.menu('root'), sub=g.menu('sub',root); g.triggers.get('root')!.closest=()=>sub; expect(resolveMenuRoot(sub)).toBeNull();
});


import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
it('pins the Bits2.20.6 runtime-only validity bridge until its public types are rechecked', () => {
 const installed = JSON.parse(readFileSync(new URL('../package.json', import.meta.resolve('bits-ui')), 'utf8'));
 expect(installed.version).toBe('2.20.6');
 const probe = fileURLToPath(new URL('./bits-validity-pin.ts', import.meta.url));
 const source = 'import type { DropdownMenu } from "bits-ui"; type Content = DropdownMenu.ContentProps; type SubContent = DropdownMenu.SubContentProps;';
 const options = { module: ts.ModuleKind.ESNext, moduleResolution: ts.ModuleResolutionKind.Bundler, skipLibCheck: true, noEmit: true };
 const host = ts.createCompilerHost(options), read = host.readFile.bind(host), exists = host.fileExists.bind(host);
 host.readFile = file => file === probe ? source : read(file);
 host.fileExists = file => file === probe || exists(file);
 const program = ts.createProgram([probe], options, host), checker = program.getTypeChecker();
 expect(ts.getPreEmitDiagnostics(program).map(d => ts.flattenDiagnosticMessageText(d.messageText, '\n'))).toEqual([]);
 const aliases = program.getSourceFile(probe)!.statements.filter(ts.isTypeAliasDeclaration);
 expect(aliases).toHaveLength(2);
 for (const alias of aliases) {
  const props = checker.getTypeAtLocation(alias);
  expect(props.getProperty('onInteractOutside')).toBeDefined();
  expect(props.getProperty('isValidEvent')).toBeUndefined();
 }
});
