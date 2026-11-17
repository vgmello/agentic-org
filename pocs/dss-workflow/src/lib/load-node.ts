// Node/Bun loader for the CLI and tests (the UI uses Vite imports in workflow.ts).
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { parse } from 'yaml';
import { BLOCK_FILE, loadFlow, type Flow, type FlowDef } from './engine.ts';

export const WORKFLOW_DIR = join(import.meta.dirname, '..', '..', 'workflow');

export function readFlowDef(): FlowDef {
	return parse(readFileSync(join(WORKFLOW_DIR, 'dss.flow.yaml'), 'utf8')) as FlowDef;
}

/** Reads only blocks/<name>.md paths; loadFlow reports anything skipped as missing/invalid. */
export function readMarkdown(def: FlowDef): Record<string, string> {
	const md: Record<string, string> = {};
	for (const b of def.blocks) {
		if (!BLOCK_FILE.test(b.file)) continue;
		try {
			md[b.file] = readFileSync(join(WORKFLOW_DIR, b.file), 'utf8');
		} catch {
			// missing file: reported by loadFlow
		}
	}
	return md;
}

export function readFlow(capabilities: string[]): Flow {
	const def = readFlowDef();
	return loadFlow(def, readMarkdown(def), capabilities);
}
