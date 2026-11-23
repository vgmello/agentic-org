// The Projects screens' words, addresses and checks, one function per fact so they are unit tested
// (work-items-ux §3, PRJ-1..5, development-processes §4, offices-and-teams-ux §7.9). The screens only arrange them.
import type {
	BugCounts,
	FeatureCounts,
	ProjectDetail,
	ProjectRow,
	TaskCounts,
	TeamRef
} from './types';

export const PROJECTS_URL = '/projects';

const count = new Intl.NumberFormat('en-US');
const plural = (n: number, one: string, many = `${one}s`) =>
	`${count.format(n)} ${n === 1 ? one : many}`;

export const projectRef = (p: Pick<ProjectRow, 'number'>) => `P-${p.number}`;

// ── Tabs (PRJ-1, PKG-15) ────────────────────────────────────────────────────────────────────────────────────────

export type TabId =
	| 'overview'
	| 'work'
	| 'features'
	| 'releases'
	| 'docs'
	| 'team'
	| 'activity'
	| 'conversations'
	| 'settings'
	| `process:${string}`;

export interface ProjectTab {
	id: TabId;
	label: string;
	/** The path under the project: '' for Overview. */
	path: string;
}

/**
 * Which experiences this release serves. A tab whose experience is not served is never shown (PRJ-1); Work is
 * served as a link to its own page, which says it isn't built yet (lead, 2026-11-19).
 */
export interface ServedTabs {
	work: boolean;
	processTabs: boolean;
	features: boolean;
	releases: boolean;
	docs: boolean;
	settings: boolean;
}

export const SERVED: ServedTabs = {
	work: true,
	processTabs: false,
	features: false,
	releases: false,
	docs: false,
	settings: true
};

/**
 * The tab bar (development-processes §4): Overview and Work; the development process's tabs in order; Features and
 * Releases when the project holds them (or has no process) and the process gave no tab for them; then Docs, Team,
 * Activity, Conversations; Settings last. Nothing about tabs is stored on the project.
 */
export function projectTabs(
	project: Pick<ProjectDetail, 'developmentProcess' | 'processTabs' | 'features' | 'releases'>,
	served: ServedTabs = SERVED
): ProjectTab[] {
	const tabs: ProjectTab[] = [{ id: 'overview', label: 'Overview', path: '' }];
	if (served.work) tabs.push({ id: 'work', label: 'Work', path: 'work' });
	const given = new Set(project.processTabs.map((t) => t.view));
	if (served.processTabs)
		for (const t of project.processTabs)
			tabs.push({ id: `process:${t.key}`, label: t.title, path: `t/${t.key}` });
	const none = project.developmentProcess === null;
	const holdsFeatures =
		project.features.planned + project.features.inProgress + project.features.live > 0;
	const needsFeatures = (none || holdsFeatures) && !(served.processTabs && given.has('features'));
	const needsReleases =
		(none || project.releases > 0) && !(served.processTabs && given.has('releases'));
	if (served.features && needsFeatures)
		tabs.push({ id: 'features', label: 'Features', path: 'features' });
	if (served.releases && needsReleases)
		tabs.push({ id: 'releases', label: 'Releases', path: 'releases' });
	if (served.docs) tabs.push({ id: 'docs', label: 'Docs', path: 'docs' });
	tabs.push(
		{ id: 'team', label: 'Team', path: 'team' },
		{ id: 'activity', label: 'Activity', path: 'activity' },
		{ id: 'conversations', label: 'Conversations', path: 'conversations' }
	);
	if (served.settings) tabs.push({ id: 'settings', label: 'Settings', path: 'settings' });
	return tabs;
}

/**
 * New project from anywhere (the top-bar New menu, Home, a team's page): the Projects page opens the New project sheet,
 * with `teamId` as the accountable team when given (offices-and-teams-ux §7.9 "prefilled from the launching team").
 */
export const newProjectUrl = (teamId?: string) =>
	`${PROJECTS_URL}?new${teamId ? `&team=${encodeURIComponent(teamId)}` : ''}`;

export const projectHref = (projectId: string, path = '') =>
	`${PROJECTS_URL}/${encodeURIComponent(projectId)}${path ? `/${path}` : ''}`;

/** The tab a path is on: the first path segment under the project; anything unknown counts as Overview. */
export function tabAt(pathname: string, projectId: string, tabs: ProjectTab[]): ProjectTab {
	const base = projectHref(projectId);
	const rest = pathname.startsWith(`${base}/`) ? pathname.slice(base.length + 1) : '';
	return (
		tabs.find((t) => t.path !== '' && (rest === t.path || rest.startsWith(`${t.path}/`))) ?? tabs[0]
	);
}

// ── Counts ──────────────────────────────────────────────────────────────────────────────────────────────────────

export const openTasks = (t: TaskCounts) => t.todo + t.inProgress + t.blocked;
/** Cancelled work is finished but not delivered, so it is left out of "of N" (features-ux progress). */
export const countedTasks = (t: TaskCounts) => openTasks(t) + t.done;

/** "5 open · 1 blocked", "No open tasks". */
export function openText(t: TaskCounts): string {
	const open = openTasks(t);
	if (open === 0) return 'No open tasks';
	return t.blocked > 0
		? `${count.format(open)} open · ${count.format(t.blocked)} blocked`
		: `${count.format(open)} open`;
}

/** "12 of 30 tasks done", "No tasks yet". */
export function doneText(t: TaskCounts): string {
	const total = countedTasks(t);
	if (total === 0) return 'No tasks yet';
	return `${count.format(t.done)} of ${plural(total, 'task')} done`;
}

/** Delivery progress percentage (0–100) from delivered tasks over counted tasks. */
export function progressPercent(t: TaskCounts): number {
	const total = countedTasks(t);
	if (total === 0) return 0;
	return Math.min(100, Math.max(0, Math.round((t.done / total) * 100)));
}

export type ProjectBarTone = 'ok' | 'warn' | 'bad' | 'accent' | 'archived';

/** Semantic progress-bar tone matching Executive Home scorecard cues. */
export function projectBarTone(
	tasks: TaskCounts,
	bugs: BugCounts,
	archived = false
): ProjectBarTone {
	if (archived) return 'archived';
	if (bugs.critical > 0) return 'bad';
	if (tasks.blocked > 0 || bugs.major > 0) return 'warn';
	const total = countedTasks(tasks);
	if (total > 0 && tasks.done >= total) return 'ok';
	return 'accent';
}

/** Short executive health label paired with `projectBarTone`. */
export function projectHealthLabel(tone: ProjectBarTone): string {
	if (tone === 'bad') return 'At risk';
	if (tone === 'warn') return 'Needs attention';
	if (tone === 'ok') return 'On track';
	if (tone === 'archived') return 'Archived';
	return 'Active';
}

export interface ProjectsPortfolioSummary {
	active: number;
	archived: number;
	openTasks: number;
	blockedTasks: number;
	openBugs: number;
	criticalBugs: number;
	attentionProjects: number;
}

/** Aggregate portfolio counts across projects for the Projects KPI strip. */
export function projectsPortfolioSummary(
	rows: readonly Pick<ProjectRow, 'archived' | 'tasks' | 'bugs'>[]
): ProjectsPortfolioSummary {
	let active = 0;
	let archived = 0;
	let openTaskCount = 0;
	let blockedTasks = 0;
	let openBugCount = 0;
	let criticalBugs = 0;
	let attentionProjects = 0;
	for (const r of rows) {
		if (r.archived) {
			archived++;
			continue;
		}
		active++;
		openTaskCount += openTasks(r.tasks);
		blockedTasks += r.tasks.blocked;
		openBugCount += openBugs(r.bugs);
		criticalBugs += r.bugs.critical;
		const tone = projectBarTone(r.tasks, r.bugs, false);
		if (tone === 'bad' || tone === 'warn') attentionProjects++;
	}
	return {
		active,
		archived,
		openTasks: openTaskCount,
		blockedTasks,
		openBugs: openBugCount,
		criticalBugs,
		attentionProjects
	};
}

const SEVERITIES: [keyof BugCounts, string][] = [
	['critical', 'critical'],
	['major', 'major'],
	['minor', 'minor'],
	['trivial', 'trivial'],
	['unset', 'not set']
];

/** The severities with open bugs, in severity order, Not set last (PRJ-5): "1 critical · 2 not set". */
export function bugParts(b: BugCounts): { key: keyof BugCounts; text: string }[] {
	return SEVERITIES.filter(([k]) => b[k] > 0).map(([k, word]) => ({
		key: k,
		text: `${count.format(b[k])} ${word}`
	}));
}

export const openBugs = (b: BugCounts) => b.critical + b.major + b.minor + b.trivial + b.unset;

/** "3 open bugs: 1 critical · 2 not set", "No open bugs". */
export function bugsText(b: BugCounts): string {
	const total = openBugs(b);
	if (total === 0) return 'No open bugs';
	return `${plural(total, 'open bug')}: ${bugParts(b)
		.map((p) => p.text)
		.join(' · ')}`;
}

/** "2 in progress · 1 planned · 5 live" (features-ux Overview tile); "No features yet". */
export function featuresText(f: FeatureCounts): string {
	const parts = [
		f.inProgress > 0 && `${count.format(f.inProgress)} in progress`,
		f.planned > 0 && `${count.format(f.planned)} planned`,
		f.live > 0 && `${count.format(f.live)} live`
	].filter((p): p is string => !!p);
	return parts.length ? parts.join(' · ') : 'No features yet';
}

/**
 * A "·"-separated line that wraps only between its parts and never strands a dot at the start of a line: spaces inside
 * a part and before each dot don't break (U+00A0), the space after a dot does.
 */
export const keepWords = (line: string) =>
	line
		.split(' · ')
		.map((part) => part.replaceAll(' ', '\u00a0'))
		.join('\u00a0· ');

export const teamNames = (teams: TeamRef[]) => teams.map((t) => t.name).join(', ');

/** Projects A–Z by name; archived ones after the active ones. */
export function sortProjects<T extends Pick<ProjectRow, 'name' | 'archived'>>(rows: T[]): T[] {
	return rows.toSorted(
		(a, b) =>
			Number(a.archived) - Number(b.archived) ||
			a.name.localeCompare(b.name, 'en', { sensitivity: 'base' })
	);
}

// ── Fields (organization-data 06: slugs; 02: integration branches) ──────────────────────────────────────────────

export const NAME_MAX = 80;
export const SLUG_MAX = 63;

export function nameProblem(name: string): string | undefined {
	const n = name.trim();
	if (!n) return 'Enter a name for the project.';
	if (n.length > NAME_MAX) return `Use at most ${NAME_MAX} characters.`;
	return undefined;
}

/** A slug suggested from a name: lowercase kebab-case, letters and digits only. */
export function slugFrom(name: string): string {
	return name
		.normalize('NFKD')
		.replace(/[̀-ͯ]/g, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, SLUG_MAX)
		.replace(/-+$/g, '');
}

export function slugProblem(slug: string): string | undefined {
	const s = slug.trim();
	if (!s) return 'Enter a slug, such as reservations.';
	if (s.length > SLUG_MAX) return `Use at most ${SLUG_MAX} characters.`;
	if (!/^[a-z0-9]+(-[a-z0-9]+)*$/.test(s))
		return 'Use lowercase letters, digits and single hyphens, starting and ending with a letter or digit.';
	return undefined;
}

/** A Git branch name as the integration branch of one repository association. */
export function branchProblem(branch: string): string | undefined {
	const b = branch.trim();
	if (!b) return 'Enter the integration branch, such as main.';
	if (
		/[\s~^:?*[\\]|\.\.|@\{|\/\/|^\/|\/$|^-|\.lock$|\.$/.test(b) ||
		[...b].some((c) => c.charCodeAt(0) < 32 || c.charCodeAt(0) === 127)
	)
		return 'That is not a valid branch name. Use letters, digits, "-", "_", "." and "/", such as release/1.4.';
	return undefined;
}

// ── The brief, as elements (never HTML) ─────────────────────────────────────────────────────────────────────────

export type BriefBlock =
	| { kind: 'heading'; text: string }
	| { kind: 'list'; ordered: boolean; items: string[] }
	| { kind: 'paragraph'; text: string };

/**
 * The brief's Markdown as headings, lists and paragraphs. Inline marks are left as typed; nothing is rendered as
 * HTML. A shared Markdown reader belongs in $lib/components once a second area needs the same subset.
 */
export function briefBlocks(markdown: string): BriefBlock[] {
	const blocks: BriefBlock[] = [];
	let paragraph: string[] = [];
	let list: { ordered: boolean; items: string[] } | undefined;
	const flush = () => {
		if (paragraph.length) blocks.push({ kind: 'paragraph', text: paragraph.join(' ') });
		paragraph = [];
		if (list) blocks.push({ kind: 'list', ...list });
		list = undefined;
	};
	for (const raw of markdown.split(/\r?\n/)) {
		const line = raw.trim();
		const heading = /^#{1,6}\s+(.*)$/.exec(line);
		const bullet = /^[-*+]\s+(.*)$/.exec(line);
		const numbered = /^\d+[.)]\s+(.*)$/.exec(line);
		if (!line) flush();
		else if (heading) {
			flush();
			blocks.push({ kind: 'heading', text: heading[1] });
		} else if (bullet || numbered) {
			const ordered = !!numbered;
			if (paragraph.length || (list && list.ordered !== ordered)) flush();
			list ??= { ordered, items: [] };
			list.items.push((bullet ?? numbered)![1]);
		} else if (list) list.items[list.items.length - 1] += ` ${line}`;
		else paragraph.push(line);
	}
	flush();
	return blocks;
}

// ── Refusals and notices ────────────────────────────────────────────────────────────────────────────────────────

export const CHANGES_PAUSED =
	"HQ can't be reached, so changes are paused. What's shown is the last copy.";
export const VIEW_ONLY =
	"Only the organization's owner or a lead above the project's accountable team can change its settings.";
export const archivedText = (name: string) =>
	`${name} is archived: its settings are kept as they were and can't be changed.`;
export const conflictText = (name: string) =>
	`${name} changed while you were editing. Here's the latest; what you typed is kept.`;
