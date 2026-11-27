export const CSS_IMPORT_RE =
	/(?:^|[\n;])\s*import\s+(?:[^'"]+\s+from\s+)?['"][^'"]+\.css(?:[?#][^'"]*)?['"]|\bimport\(\s*['"][^'"]+\.css(?:[?#][^'"]*)?['"]\s*\)/;

const STYLE_BLOCK_RE = /<style\b[^>]*>([\s\S]*?)<\/style>/gi;
const CSS_AT_IMPORT_RE = /@import\b/i;

export function findDisallowedCssUsage(rel: string, source: string): string | undefined {
	if (CSS_IMPORT_RE.test(source)) {
		return `${rel}: imports a plain .css file (keep component styles scoped in <style> blocks)`;
	}
	if (rel.endsWith('.svelte')) {
		STYLE_BLOCK_RE.lastIndex = 0;
		let match: RegExpExecArray | null;
		while ((match = STYLE_BLOCK_RE.exec(source)) !== null) {
			if (CSS_AT_IMPORT_RE.test(match[1] ?? '')) {
				return `${rel}: uses @import inside <style> (keep component styles scoped without external .css imports)`;
			}
		}
	}
	return undefined;
}
