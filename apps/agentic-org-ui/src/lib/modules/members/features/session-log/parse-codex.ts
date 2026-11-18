// Proof-of-concept parser for Codex's rollout file (one `{timestamp, type, payload}` object per line), tested
// against the fixture in e2e/fakes/data/members/sessions. Unknown lines become `raw` entries (REQ-05).
import { diffFromPatch, type FileAction, type SessionEntry } from './entries';

type Json = Record<string, unknown>;
const isObj = (x: unknown): x is Json => typeof x === 'object' && x !== null && !Array.isArray(x);
const str = (x: unknown) => (typeof x === 'string' ? x : undefined);
const num = (x: unknown) => (typeof x === 'number' && Number.isFinite(x) ? x : undefined);

function parseJson(text: string | undefined): unknown {
	if (text === undefined) return undefined;
	try {
		return JSON.parse(text);
	} catch {
		return undefined;
	}
}

/** The files an apply_patch body touches, with each one's change. */
export function filesFromPatch(
	patch: string
): { path: string; action: FileAction; lines: string[] }[] {
	const files: { path: string; action: FileAction; lines: string[] }[] = [];
	for (const line of patch.split('\n')) {
		const header = /^\*\*\* (Add|Update|Delete) File: (.+)$/.exec(line);
		if (header) {
			files.push({
				path: header[2].trim(),
				action: header[1] === 'Add' ? 'create' : header[1] === 'Delete' ? 'delete' : 'edit',
				lines: []
			});
		} else if (files.length > 0 && !line.startsWith('***'))
			files[files.length - 1].lines.push(line);
	}
	return files;
}

function commandText(args: unknown): string | undefined {
	if (!isObj(args)) return undefined;
	const command = args.command;
	if (Array.isArray(command)) return command.map(String).join(' ');
	return str(command);
}

/** Parse the lines of one Codex rollout file into entries, in file order. */
export function parseCodex(lines: unknown[]): {
	entries: SessionEntry[];
	toolVersion?: string;
	startedAt?: string;
} {
	const entries: SessionEntry[] = [];
	let toolVersion: string | undefined;
	let startedAt: string | undefined;
	lines.forEach((line, index) => {
		const id = `l${index}`;
		const raw = () => entries.push({ id, kind: 'raw', source: JSON.stringify(line) });
		if (!isObj(line) || !isObj(line.payload)) return raw();
		const at = str(line.timestamp);
		const payload = line.payload;
		const kind = str(payload.type);
		if (line.type === 'session_meta') {
			toolVersion = str(payload.cli_version);
			startedAt = str(payload.timestamp) ?? at;
			return entries.push({
				id,
				at,
				kind: 'notice',
				text: `Session started in ${str(payload.cwd) ?? 'its home'}`
			});
		}
		if (line.type === 'turn_context') return;
		if (line.type === 'event_msg') {
			if (kind === 'token_count') {
				const info = isObj(payload.info) ? payload.info : undefined;
				const last = info && isObj(info.last_token_usage) ? info.last_token_usage : undefined;
				if (!last) return entries.push({ id, at, kind: 'usage', complete: false });
				const inputTokens = num(last.input_tokens);
				const outputTokens = num(last.output_tokens);
				return entries.push({
					id,
					at,
					kind: 'usage',
					inputTokens,
					outputTokens,
					cachedTokens: num(last.cached_input_tokens),
					complete: inputTokens !== undefined && outputTokens !== undefined
				});
			}
			// user_message / agent_message repeat the response items; the response items are the record.
			if (kind === 'user_message' || kind === 'agent_message' || kind === 'agent_reasoning') return;
			return raw();
		}
		if (line.type !== 'response_item') return raw();
		if (kind === 'reasoning') return;
		if (kind === 'message') {
			const role =
				payload.role === 'user' ? 'user' : payload.role === 'assistant' ? 'assistant' : 'system';
			const text = Array.isArray(payload.content)
				? payload.content
						.filter(isObj)
						.map((c) => str(c.text) ?? '')
						.join('\n')
				: '';
			return entries.push({ id, at, kind: 'message', role, text });
		}
		if (kind === 'function_call') {
			const name = str(payload.name) ?? 'tool';
			const args = parseJson(str(payload.arguments));
			const command = commandText(args);
			entries.push({
				id,
				at,
				kind: 'tool_call',
				callId: str(payload.call_id),
				tool: name,
				summary: command ?? name,
				input: str(payload.arguments) ?? ''
			});
			if (command?.startsWith('apply_patch') && isObj(args) && Array.isArray(args.command))
				filesFromPatch(String(args.command[1] ?? '')).forEach((f, i) =>
					entries.push({
						id: `${id}.f${i}`,
						at,
						kind: 'file',
						path: f.path,
						action: f.action,
						diff: f.action === 'delete' ? undefined : diffFromPatch(f.lines)
					})
				);
			return;
		}
		if (kind === 'custom_tool_call') {
			const name = str(payload.name) ?? 'tool';
			const input = str(payload.input) ?? '';
			const files = name === 'apply_patch' ? filesFromPatch(input) : [];
			entries.push({
				id,
				at,
				kind: 'tool_call',
				callId: str(payload.call_id),
				tool: name,
				summary: files.length ? `apply_patch ${files.map((f) => f.path).join(', ')}` : name,
				input
			});
			files.forEach((f, i) =>
				entries.push({
					id: `${id}.f${i}`,
					at,
					kind: 'file',
					path: f.path,
					action: f.action,
					diff: f.action === 'delete' ? undefined : diffFromPatch(f.lines)
				})
			);
			return;
		}
		if (kind === 'function_call_output' || kind === 'custom_tool_call_output') {
			const parsed = parseJson(str(payload.output));
			const output = isObj(parsed) ? (str(parsed.output) ?? '') : (str(payload.output) ?? '');
			const exit =
				isObj(parsed) && isObj(parsed.metadata) ? num(parsed.metadata.exit_code) : undefined;
			return entries.push({
				id,
				at,
				kind: 'tool_result',
				callId: str(payload.call_id),
				ok: exit === undefined || exit === 0,
				output
			});
		}
		raw();
	});
	return { entries, toolVersion, startedAt };
}
