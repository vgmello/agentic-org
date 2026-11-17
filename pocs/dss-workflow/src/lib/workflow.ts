import { parse } from 'yaml';
import { loadFlow, type Flow, type FlowDef } from './engine';
import source from '../../workflow/dss.flow.yaml?raw';

const raw = import.meta.glob('../../workflow/blocks/*.md', {
	query: '?raw',
	import: 'default',
	eager: true
}) as Record<string, string>;

const markdownByFile = Object.fromEntries(
	Object.entries(raw).map(([path, md]) => [path.replace('../../workflow/', ''), md])
);

export const flowDef = parse(source) as FlowDef;

export const makeFlow = (capabilities: string[]): Flow => loadFlow(flowDef, markdownByFile, capabilities);
