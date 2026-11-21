// Safe per-effective-usage-record CSV export (costs-and-insights-ux §3, INS-5).
// Escapes spreadsheet formula-injection prefixes (=, +, -, @, \t, \r) on every text cell and obeys RFC 4180 quoting.
import type { Coverage, UsageRecord } from './types';

const FORMULA_PREFIX = /^[=+\-@\t\r\n]/;
const LEADING_SPACE_FORMULA = /^\s+[=+\-@]/;

/**
 * Neutralizes spreadsheet formula injection (CSV/DDE injection) and formats a single RFC 4180 cell.
 * Prefixing a single quote prevents spreadsheet engines from evaluating `=`, `+`, `-`, `@`, `\t`, or `\r` as formulas.
 */
export function escapeCsvCell(value: string | number | boolean | null | undefined): string {
	if (value === null || value === undefined) return '';
	if (typeof value === 'boolean') return value ? 'true' : 'false';
	if (typeof value === 'number') {
		if (!Number.isFinite(value)) return '';
		const numStr = String(value);
		return numStr.startsWith('-') ? `"'${numStr}"` : numStr;
	}
	let safe = value;
	if (FORMULA_PREFIX.test(safe) || LEADING_SPACE_FORMULA.test(safe)) {
		safe = `'${safe}`;
	}
	if (/[",\r\n]/.test(safe)) {
		return `"${safe.replaceAll('"', '""')}"`;
	}
	return safe;
}

/** Exact micro-USD to plain decimal USD string without floating-point arithmetic ("12.345000"). */
function exactUsdFromMicro(microUsd: number | null): string {
	if (microUsd === null) return '';
	const whole = Math.floor(Math.abs(microUsd) / 1_000_000);
	const frac = String(Math.abs(microUsd) % 1_000_000).padStart(6, '0');
	return `${microUsd < 0 ? '-' : ''}${whole}.${frac}`;
}

export const USAGE_CSV_HEADERS = [
	'event_id',
	'occurred_at',
	'consumer_kind',
	'consumer_id',
	'consumer_name',
	'runtime',
	'provider',
	'model',
	'project_id',
	'project_name',
	'team_id',
	'team_name',
	'task_ref',
	'task_title',
	'attribution_basis',
	'work_type',
	'tokens_input',
	'tokens_output',
	'tokens_cache_read',
	'tokens_cache_write',
	'tokens_reasoning',
	'token_status',
	'cost_micro_usd',
	'cost_usd',
	'cost_basis',
	'cost_status',
	'api_equivalent',
	'price_id',
	'price_label',
	'issue',
	'correction_by',
	'correction_at',
	'correction_reason',
	'coverage_checkpoint_at'
] as const;

/** Builds an RFC 4180 CSV document with CRLF line endings, one row per effective usage record. */
export function buildUsageCsv(records: readonly UsageRecord[], coverage: Coverage): string {
	const lines: string[] = [USAGE_CSV_HEADERS.join(',')];
	for (const r of records) {
		const row = [
			r.id,
			r.occurredAt,
			r.consumer.kind,
			r.consumer.kind === 'agent' ? r.consumer.memberId : '',
			r.consumer.kind === 'agent' ? r.consumer.name : r.consumer.label,
			r.runtime,
			r.provider ?? '',
			r.model ?? '',
			r.attribution.project?.id ?? '',
			r.attribution.project?.name ?? '',
			r.attribution.team?.id ?? '',
			r.attribution.team?.name ?? '',
			r.attribution.task?.ref ?? '',
			r.attribution.task?.title ?? '',
			r.attribution.basis,
			r.workType ?? '',
			r.tokens?.input ?? '',
			r.tokens?.output ?? '',
			r.tokens?.cacheRead ?? '',
			r.tokens?.cacheWrite ?? '',
			r.tokens?.reasoning ?? '',
			r.tokenStatus,
			r.costMicroUsd ?? '',
			exactUsdFromMicro(r.costMicroUsd),
			r.costBasis,
			r.costStatus,
			r.apiEquivalent,
			r.price?.id ?? '',
			r.price?.label ?? '',
			r.issue ?? '',
			r.correction?.byName ?? '',
			r.correction?.at ?? '',
			r.correction?.reason ?? '',
			coverage.checkpointAt
		];
		lines.push(row.map(escapeCsvCell).join(','));
	}
	return `${lines.join('\r\n')}\r\n`;
}

/** Triggers a browser file download of the CSV text and revokes the temporary object URL. */
export function downloadCsv(filename: string, csvText: string): void {
	const safeName = filename.replace(/[^a-zA-Z0-9._-]+/g, '-');
	const blob = new Blob([csvText], { type: 'text/csv;charset=utf-8' });
	const url = URL.createObjectURL(blob);
	const link = document.createElement('a');
	link.href = url;
	link.download = safeName.endsWith('.csv') ? safeName : `${safeName}.csv`;
	document.body.append(link);
	link.click();
	link.remove();
	setTimeout(() => URL.revokeObjectURL(url), 1000);
}
