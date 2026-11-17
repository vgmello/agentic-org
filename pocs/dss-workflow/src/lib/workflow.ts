import { loadFlow, type FlowDef } from './engine';
import def from '../../workflow/dss.flow.json';

const raw = import.meta.glob('../../workflow/blocks/*.md', {
	query: '?raw',
	import: 'default',
	eager: true
}) as Record<string, string>;

const markdownByFile = Object.fromEntries(
	Object.entries(raw).map(([path, md]) => [path.replace('../../workflow/', ''), md])
);

export const flow = loadFlow(def as FlowDef, markdownByFile);
