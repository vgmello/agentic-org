// Proof-of-concept parser for Claude Code's conversation file (one JSON object per line), tested against the
// fixture in e2e/fakes/data/members/sessions. In the product the office parses and redacts; this proves the entry
// model and the view (REQ-05). A line it does not recognise becomes a `raw` entry.
import { diffFromReplace, type DiffLine, type FileAction, type SessionEntry } from './entries';

type Json = Record<string, unknown>;
const isObj = (x: unknown): x is Json => typeof x === 'object' && x !== null && !Array.isArray(x);
const str = (x: unknown) => (typeof x === 'string' ? x : undefined);
const num = (x: unknown) => (typeof x === 'number' && Number.isFinite(x) ? x : undefined);

/** The tools whose input names a file, and what they do to it. */
const FILE_TOOLS: Record<string, FileAction> = {
	Read: 'read',
	Edit: 'edit',
	MultiEdit: 'edit',
	Write: 'create',
	NotebookEdit: 'edit'
};

function toolSummary(name: string, input: Json): string {
	const path = str(input.file_path) ?? str(input.notebook_path) ?? str(input.path);
	if (path) return `${name} ${path}`;
	const command = str(input.command);
	if (command) return command.split('\n')[0];
	const pattern = str(input.pattern);
	if (pattern) return `${name} ${pattern}`;
	return name;
}

function fileDiff(name: string, input: Json): DiffLine[] | undefined {
	if (name === 'Edit') return diffFromReplace(str(input.old_string) ?? '', str(input.new_string) ?? '');
	if (name === 'MultiEdit' && Array.isArray(input.edits))
		return input.edits.filter(isObj).flatMap((e) => diffFromReplace(str(e.old_string) ?? '', str(e.new_string) ?? ''));
	if (name === 'Write') return diffFromReplace('', str(input.content) ?? '');
	return undefined;
}

function resultText(content: unknown): string {
	if (typeof content === 'string') return content;
	if (Array.isArray(content))
		return content
			.filter(isObj)
			.map((c) => str(c.text) ?? '')
			.join('\n');
	return '';
}

/** Parse the lines of one Claude Code conversation file into entries, in file order. */
export function parseClaude(lines: unknown[]): { entries: SessionEntry[]; toolVersion?: string; startedAt?: string } {
	const entries: SessionEntry[] = [];
	let toolVersion: string | undefined;
	let startedAt: string | undefined;
	lines.forEach((line, index) => {
		const id = `l${index}`;
		const raw = () => entries.push({ id, kind: 'raw', source: JSON.stringify(line) });
		if (!isObj(line)) return raw();
		const at = str(line.timestamp);
		startedAt ??= at;
		toolVersion ??= str(line.version);
		const type = str(line.type);
		if (type === 'summary') return entries.push({ id, at, kind: 'notice', text: `Summary: ${str(line.summary) ?? ''}` });
		if (type === 'system') return entries.push({ id, at, kind: 'notice', text: str(line.content) ?? 'System notice' });
		if (type !== 'user' && type !== 'assistant') return raw();
		const message = line.message;
		if (!isObj(message)) return raw();
		const role = type;
		const content = message.content;
		if (typeof content === 'string') {
			entries.push({ id, at, kind: 'message', role, text: content });
		} else if (Array.isArray(content)) {
			content.forEach((part, p) => {
				const pid = `${id}.${p}`;
				if (!isObj(part)) return entries.push({ id: pid, at, kind: 'raw', source: JSON.stringify(part) });
				const kind = str(part.type);
				if (kind === 'text') entries.push({ id: pid, at, kind: 'message', role, text: str(part.text) ?? '' });
				else if (kind === 'thinking') return;
				else if (kind === 'tool_use') {
					const name = str(part.name) ?? 'tool';
					const input = isObj(part.input) ? part.input : {};
					entries.push({ id: pid, at, kind: 'tool_call', callId: str(part.id), tool: name, summary: toolSummary(name, input), input: JSON.stringify(input, null, 2) });
					const path = str(input.file_path) ?? str(input.notebook_path);
					if (path && FILE_TOOLS[name]) entries.push({ id: `${pid}.f`, at, kind: 'file', path, action: FILE_TOOLS[name], diff: fileDiff(name, input) });
				} else if (kind === 'tool_result')
					entries.push({ id: pid, at, kind: 'tool_result', callId: str(part.tool_use_id), ok: part.is_error !== true, output: resultText(part.content) });
				else entries.push({ id: pid, at, kind: 'raw', source: JSON.stringify(part) });
			});
		} else return raw();
		const usage = isObj(message.usage) ? message.usage : undefined;
		if (role === 'assistant' && usage) {
			const inputTokens = num(usage.input_tokens);
			const outputTokens = num(usage.output_tokens);
			entries.push({ id: `${id}.u`, at, kind: 'usage', inputTokens, outputTokens, cachedTokens: num(usage.cache_read_input_tokens), complete: inputTokens !== undefined && outputTokens !== undefined });
		}
	});
	return { entries, toolVersion, startedAt };
}
