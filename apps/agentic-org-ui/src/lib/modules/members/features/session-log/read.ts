// The session log as the viewer reads it: the run's conversation file from the api, parsed by the parser for its
// format into the one entry model (REQ-05 "One event model, two parsers"). In the product the office parses and
// redacts; then only this file changes, and the view keeps reading a `SessionLog`.
import type { MembersApi } from '../../api';
import type { SessionFile } from '../../types';
import type { SessionLog } from './entries';
import { parseClaude } from './parse-claude';
import { parseCodex } from './parse-codex';

export function parseSessionFile(file: SessionFile): SessionLog {
	const parsed = file.format === 'claude' ? parseClaude(file.lines) : parseCodex(file.lines);
	return { runId: file.runId, memberId: file.memberId, format: file.format, ...parsed };
}

export async function readSessionLog(
	api: Pick<MembersApi, 'sessionFile'>,
	organizationId: string,
	memberId: string,
	runId: string
): Promise<SessionLog> {
	return parseSessionFile(await api.sessionFile(organizationId, memberId, runId));
}

/** The runtime's name for the header: "Claude Code 2.1.4", "Codex". */
export const formatName = (log: Pick<SessionLog, 'format' | 'toolVersion'>) =>
	`${log.format === 'claude' ? 'Claude Code' : 'Codex'}${log.toolVersion ? ` ${log.toolVersion}` : ''}`;

/** Long output is cut to `limit` lines for the row; the rest is one click away. */
export function previewLines(text: string, limit: number): { shown: string; hidden: number } {
	const lines = text.split('\n');
	if (lines.length <= limit) return { shown: text, hidden: 0 };
	return { shown: lines.slice(0, limit).join('\n'), hidden: lines.length - limit };
}
