import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { loadFlow, type FlowDef } from './engine';

const root = join(import.meta.dirname, '..', '..', 'workflow');
const def = JSON.parse(readFileSync(join(root, 'dss.flow.json'), 'utf8')) as FlowDef;
const md = Object.fromEntries(def.blocks.map((b) => [b.file, readFileSync(join(root, b.file), 'utf8')]));
const flow = loadFlow(def, md);

describe('dss flow', () => {
	it('command returns the start block markdown (REQ-01)', () => {
		const a = flow.runCommand('project-setup');
		expect(a.block.id).toBe('project-setup');
		expect(a.markdown).toContain('Tech stack evaluation');
	});

	it('picks autonomous or interactive Technical DSS from standards.defined', () => {
		expect(flow.fire('publish-product-dss', 'product-dss.published', { 'standards.defined': true }).block.id).toBe(
			'tech-dss-autonomous'
		);
		expect(flow.fire('publish-product-dss', 'product-dss.published', {}).block.id).toBe('tech-dss-interactive');
	});

	it('hands the block to a different agent on status:in_review (REQ-02)', () => {
		const a = flow.fire('execute-tasks', 'status:in_review', {});
		expect(a.block.id).toBe('code-review');
		expect(a.agent.id).toBe('reviewer');
		expect(a.handoff).toBe(true);
	});

	it('keeps the same agent without a handoff', () => {
		const a = flow.fire('product-dss-dialogue', 'context.sufficient', {});
		expect(a.agent.id).toBe('product-owner');
		expect(a.handoff).toBe(false);
		expect(flow.runCommand('dss').handoff).toBe(false);
	});

	it('routes the optional standards byproduct and autonomous escalation', () => {
		expect(flow.fire('tech-dss-autonomous', 'tech-dss.recorded', { 'standards.changed': true }).block.id).toBe(
			'architecture-standards'
		);
		expect(flow.fire('tech-dss-autonomous', 'tech-dss.recorded', {}).block.id).toBe('implementation-planning');
		expect(flow.fire('tech-dss-autonomous', 'decision.open', {}).block.id).toBe('tech-dss-interactive');
	});

	it('runs remaining tasks before reconciling', () => {
		expect(flow.fire('code-review', 'status:approved', { 'tasks.remaining': true }).block.id).toBe('execute-tasks');
		expect(flow.fire('code-review', 'status:approved', {}).block.id).toBe('reconcile');
	});

	it('loops reconcile back on drift and finishes on convergence', () => {
		expect(flow.fire('reconcile', 'drift.detected', {}).block.id).toBe('implementation-planning');
		expect(flow.fire('reconcile', 'converged', {}).block.id).toBe('converged');
		expect(flow.fire('converged', 'product-dss.new-version', {}).block.id).toBe('product-dss-dialogue');
	});

	it('ignores prototype keys in guards', () => {
		const ctx = Object.create({ 'standards.defined': true });
		expect(flow.fire('publish-product-dss', 'product-dss.published', ctx).block.id).toBe('tech-dss-interactive');
	});

	it('rejects unknown events and broken wiring', () => {
		expect(() => flow.runCommand('nope')).toThrow(/Unknown command/);
		expect(() => flow.fire('execute-tasks', 'status:approved', {})).toThrow(/No transition/);
		const noGuardPasses = { ...def, transitions: [{ from: 'converged', on: 'z', to: 'reconcile', when: 'x' }] };
		expect(() => loadFlow(noGuardPasses, md).fire('converged', 'z', {})).toThrow(/Guard blocked/);
		const broken = { ...def, transitions: [...def.transitions, { from: 'converged', on: 'x', to: 'nope' }] };
		expect(() => loadFlow(broken, md)).toThrow(/unknown to/);
		expect(() => loadFlow(def, {})).toThrow(/missing file/);
		const block = def.blocks[0];
		const variants: [Partial<FlowDef>, RegExp][] = [
			[{ blocks: [...def.blocks, block] }, /duplicate block/],
			[{ triggers: [...def.triggers, def.triggers[0]] }, /duplicate command/],
			[{ blocks: [{ ...block, agent: 'ghost' }, ...def.blocks.slice(1)] }, /unknown agent/],
			[{ blocks: [{ ...block, file: '../../package.json' }, ...def.blocks.slice(1)] }, /file must be/],
			[{ triggers: [{ command: 'x', start: 'ghost' }] }, /unknown block/],
			[{ transitions: [...def.transitions, { from: 'code-review', on: 'status:approved', to: 'converged' }] }, /alternatives/],
			[{ transitions: [...def.transitions, { from: 'converged', on: 'w', to: 'reconcile', when: 'a' }, { from: 'converged', on: 'w', to: 'reconcile', when: 'b' }] }, /alternatives/],
			[{ transitions: [...def.transitions, { from: 'converged', on: 'y', to: 'reconcile', when: '!!x' }] }, /bad guard/]
		];
		for (const [patch, error] of variants) expect(() => loadFlow({ ...def, ...patch }, md)).toThrow(error);
	});
});
