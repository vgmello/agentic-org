/** A named place the navigation links to (a project or a team), by its stable id. */
export interface NavPlace {
	id: string;
	name: string;
}

/**
 * What the shell's navigation shows beside its fixed destinations. `inboxOpen` is null while the
 * Inbox has no source (the Inbox item is then hidden); Home's "Open Inbox (N)" reads the same value.
 */
export interface ShellNavigation {
	inboxOpen: number | null;
	/** Every project, any order: the navigation lists the first five names A–Z. */
	projects: NavPlace[];
	/** Every team, any order: the navigation lists the first five names A–Z. */
	teams: NavPlace[];
}

export type SearchGroup = 'projects' | 'work' | 'members';

/** One top-bar search result. `href` is where choosing it goes. */
export interface SearchHit {
	id: string;
	title: string;
	/** A short second line: a project key, a work item's reference and state, a member's kind. */
	meta?: string;
	href: string;
}

/** Top-bar search results, grouped in the order the spec names them (app-shell-ux §0.2). */
export type SearchResults = Record<SearchGroup, SearchHit[]>;
