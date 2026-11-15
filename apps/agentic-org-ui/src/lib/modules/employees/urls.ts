/** Match the native input's 200 UTF-16 code-unit limit, after trimming outside whitespace. */
export const AGENTS_QUERY_LIMIT = 200;
export const normalizeAgentsQuery = (query: string): string => query.trim().slice(0, AGENTS_QUERY_LIMIT);

export function agentsSearchUrl(query: string): string {
	const q = normalizeAgentsQuery(query);
	return q ? `/agents?${new URLSearchParams({ q })}` : '/agents';
}

export const agentsAddUrl = (): string => '/agents?add=1';

/** Consume only a single exact intent; malformed/duplicate values are stripped without opening. */
export function consumeAgentsAddIntent(url: URL, authority: { canAdd: boolean; alreadyOpen: boolean }): { url: URL; open: boolean } | undefined {
	const values = url.searchParams.getAll('add');
	if (values.length === 0) return;
	const next = new URL(url);
	next.searchParams.delete('add');
	return { url: next, open: values.length === 1 && values[0] === '1' && authority.canAdd && !authority.alreadyOpen };
}
