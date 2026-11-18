/**
 * The slug the form offers for a name: lowercase kebab-case, as the server's rule requires
 * (org-proto `valid_slug`: at most 64 of a-z, 0-9 and single hyphens, none at either end). The server is
 * the authority; the slug is always sent explicitly and never recalculated after a rename.
 */
export const SLUG_MAX_CHARS = 64;

export function slugFromName(name: string): string {
	return name
		.normalize('NFKD')
		.replace(/\p{M}+/gu, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, SLUG_MAX_CHARS)
		.replace(/-+$/, '');
}

export const validSlug = (slug: string): boolean =>
	slug.length <= SLUG_MAX_CHARS && /^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(slug);

/**
 * The slug field follows the name until the person edits it. Clearing the slug field
 * hands it back to the name.
 */
export function createSlugField() {
	let edited = false;
	let value = '';
	return {
		get value() {
			return value;
		},
		get edited() {
			return edited;
		},
		nameChanged(name: string): string {
			if (!edited) value = slugFromName(name);
			return value;
		},
		slugEdited(slug: string, name: string): string {
			edited = slug !== '';
			value = edited ? slug : slugFromName(name);
			return value;
		}
	};
}
