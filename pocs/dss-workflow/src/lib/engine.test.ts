import { describe, expect, it } from 'vitest';
import { loadFlow, type FlowDef } from './engine';
import { readFlowDef, readMarkdown } from './load-node';

const def = readFlowDef();
const md = readMarkdown(def);
const withTransitions = (transitions: FlowDef['transitions']): FlowDef => ({ ...def, transitions });

describe('flow definition', () => {
	it('uses native rules when the host provides the capabilities', () => {
		const flow = loadFlow(def, md, ['task', 'project']);
		const toReview = flow.transitions.find((t) => t.from === 'execute-tasks' && t.to === 'code-review')!;
		expect(toReview).toMatchObject({ mode: 'native', capability: 'task', when: "task.status == 'in_review'" });
		expect(toReview.on).toBeUndefined();
	});

	it('falls back per transition when a capability is missing', () => {
		const flow = loadFlow(def, md, []);
		const toReview = flow.transitions.find((t) => t.from === 'execute-tasks' && t.to === 'code-review')!;
		expect(toReview).toMatchObject({ mode: 'fallback', capability: 'task', on: 'submitted_for_review' });
		expect(flow.outcomes('execute-tasks')).toContain('submitted_for_review');
		expect(loadFlow(def, md, ['task', 'project']).outcomes('execute-tasks')).not.toContain('submitted_for_review');
	});

	it('mixes capabilities: project native, task fallback', () => {
		const flow = loadFlow(def, md, ['project']);
		expect(flow.transitions.find((t) => t.to === 'tech-dss-autonomous')!.mode).toBe('native');
		expect(flow.transitions.find((t) => t.to === 'code-review')!.mode).toBe('fallback');
	});

	it('type-checks CEL when the flow is loaded', () => {
		const base = { from: 'reconcile', to: 'converged' };
		const cases: [FlowDef['transitions'][number], RegExp][] = [
			[{ ...base, when: "task.stauts == 'done'" }, /invalid condition/],
			[{ ...base, when: 'task.status == 1' }, /invalid condition/],
			[{ ...base, when: 'flags.nope' }, /invalid condition/],
			[{ ...base, on: 'x', where: 'event.to ==' }, /invalid condition/],
			[{ ...base, when: "task.status == 'done'" }, /needs capability task and has no fallback/],
			[{ ...base, on: 'task.status_changed' }, /needs capability task/],
			[{ ...base, when: "task.status == 'done'", fallback: { when: 'task.status == 2' } }, /fallback is not valid/],
			[{ ...base, where: "event.to == 'x'" }, /'where' needs an 'on'/],
			[{ ...base }, /needs 'on' or a level 'when'/]
		];
		for (const [t, error] of cases) expect(() => loadFlow(withTransitions([...def.transitions, t]), md, [])).toThrow(error);
	});

	it('rejects broken wiring', () => {
		expect(() => loadFlow(def, {}, [])).toThrow(/missing file/);
		expect(() => loadFlow(def, md, ['crm'])).toThrow(/unknown capability crm/);
		expect(() => loadFlow({ ...def, blocks: [...def.blocks, def.blocks[0]] }, md, [])).toThrow(/duplicate block/);
		expect(() => loadFlow({ ...def, blocks: [{ ...def.blocks[0], file: '../x.md' }, ...def.blocks.slice(1)] }, md, [])).toThrow(/file must be/);
		const twoUnconditional = withTransitions([...def.transitions, { from: 'project-setup', on: 'completed', to: 'converged' }]);
		expect(() => loadFlow(twoUnconditional, md, [])).toThrow(/2 unconditional transitions on completed/);
	});

	it('allows only the flags a transition reads', () => {
		const flow = loadFlow(def, md, ['task', 'project']);
		expect(flow.flagsFor('tech-dss-autonomous', 'recorded')).toEqual(['standards_changed']);
		expect(flow.flagsFor('publish-product-dss', 'published')).toEqual([]);
		expect(loadFlow(def, md, []).flagsFor('publish-product-dss', 'published')).toEqual(['standards_defined']);
	});
});
