import { badgeInitials as initialsOf } from '$lib/format';
import type { ActivityEntry } from '$lib/modules/members';
import type { FeedPhase, LineEventEntry, StreamEntry, StreamFilterTab } from './assembly-model';
import { extractWorkRefsFromText } from './assembly-model';

export function scrollPanelByPage(el: HTMLElement | null): void {
	if (!el) return;
	const step = Math.max(180, Math.round(el.clientHeight * 0.72));
	const reduced =
		typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
	el.scrollBy({ top: step, behavior: reduced ? 'auto' : 'smooth' });
}

export function observeScrollOverflow(el: HTMLElement, onCheck: () => void): () => void {
	onCheck();
	el.addEventListener('scroll', onCheck, { passive: true });
	const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(onCheck) : undefined;
	const observeChildren = () => {
		if (!ro) return;
		ro.observe(el);
		for (const child of el.children) ro.observe(child);
	};
	observeChildren();
	const mo =
		typeof MutationObserver !== 'undefined'
			? new MutationObserver(() => {
					observeChildren();
					onCheck();
				})
			: undefined;
	mo?.observe(el, { childList: true, subtree: true });
	const raf = requestAnimationFrame(onCheck);
	return () => {
		cancelAnimationFrame(raf);
		el.removeEventListener('scroll', onCheck);
		ro?.disconnect();
		mo?.disconnect();
	};
}

export function attachPanelEscapeListener(isOpen: () => boolean, onClose: () => void): () => void {
	const onEscapeKey = (event: KeyboardEvent) => {
		if (
			event.defaultPrevented ||
			event.isComposing ||
			event.keyCode === 229 ||
			event.key !== 'Escape' ||
			event.repeat ||
			!isOpen()
		) {
			return;
		}
		const target = event.target;
		if (
			!(target instanceof Element) ||
			!target.closest('#activity-release-panel, .activity-tools')
		) {
			return;
		}
		event.preventDefault();
		onClose();
	};
	window.addEventListener('keydown', onEscapeKey);
	return () => window.removeEventListener('keydown', onEscapeKey);
}

export function extractWorkErrorMessage(err: unknown): string {
	if (err instanceof Error && err.message) return err.message;
	if (
		typeof err === 'object' &&
		err !== null &&
		'message' in err &&
		typeof (err as { message: unknown }).message === 'string'
	) {
		return (err as { message: string }).message;
	}
	return 'Could not load work items from the office.';
}

function hasModifierKey(event: KeyboardEvent): boolean {
	return event.altKey || event.ctrlKey || event.metaKey || event.shiftKey;
}

function nextTabIndex(key: string, index: number, length: number): number | undefined {
	if (key === 'Home') return 0;
	if (key === 'End') return length - 1;
	if (key === 'ArrowRight') return (index + 1) % length;
	if (key === 'ArrowLeft') return (index - 1 + length) % length;
	return undefined;
}

export function moveProjectViewTabFocus(event: KeyboardEvent): 'assembly' | 'feed' | undefined {
	if (hasModifierKey(event)) return undefined;
	const current = event.currentTarget as HTMLButtonElement | null;
	const group = current?.closest('[role="tablist"], .activity-view-tabs');
	if (!current || !group) return undefined;
	const buttons = [...group.querySelectorAll<HTMLButtonElement>('[data-project-view]')];
	const index = buttons.indexOf(current);
	if (index < 0 || buttons.length === 0) return undefined;
	const nextIdx = nextTabIndex(event.key, index, buttons.length);
	if (nextIdx === undefined) return undefined;
	event.preventDefault();
	const nextBtn = buttons[nextIdx];
	nextBtn?.focus();
	return nextBtn?.dataset.projectView as 'assembly' | 'feed' | undefined;
}

export const STREAM_TABS: readonly { id: StreamFilterTab; label: string }[] = [
	{ id: 'all', label: 'All' },
	{ id: 'needs_you', label: 'Needs you' },
	{ id: 'work', label: 'Work' },
	{ id: 'run', label: 'Runs' },
	{ id: 'decision', label: 'Decisions' },
	{ id: 'conversation', label: 'Chat' },
	{ id: 'org', label: 'Org' }
];

export function projectEntryNeedsYou(
	entry: ActivityEntry,
	workPhase: FeedPhase,
	inboxPhase: FeedPhase,
	actionableWorkRefs: ReadonlySet<string>,
	openInboxWorkRefs: ReadonlySet<string>
): boolean {
	const refs = extractWorkRefsFromText(entry.text);
	if (refs.length === 0) return false;
	if (workPhase === 'ready' && refs.some((ref) => actionableWorkRefs.has(ref))) return true;
	if (inboxPhase === 'ready' && refs.some((ref) => openInboxWorkRefs.has(ref))) return true;
	return false;
}

export function toneFromProjectEntry(
	entry: ActivityEntry,
	needsYou: boolean
): LineEventEntry['tone'] {
	if (needsYou) return 'warn';
	const lower = `${entry.kind} ${entry.text}`.toLowerCase();
	if (lower.includes('fail') || lower.includes('block') || lower.includes('denied')) return 'bad';
	if (
		lower.includes('done') ||
		lower.includes('merg') ||
		lower.includes('finish') ||
		lower.includes('approved')
	) {
		return 'ok';
	}
	return '';
}

export function buildExtraLineEvents(
	extraProjectActivity: readonly ActivityEntry[],
	workPhase: FeedPhase,
	inboxPhase: FeedPhase,
	actionableWorkRefs: ReadonlySet<string>,
	openInboxWorkRefs: ReadonlySet<string>
): LineEventEntry[] {
	return extraProjectActivity.map((entry) => {
		const needsYou = projectEntryNeedsYou(
			entry,
			workPhase,
			inboxPhase,
			actionableWorkRefs,
			openInboxWorkRefs
		);
		return {
			id: `proj-act-${entry.id}`,
			at: entry.at,
			tone: toneFromProjectEntry(entry, needsYou),
			text: entry.actor ? `${entry.actor} ${entry.text}` : entry.text
		};
	});
}

export function mergeProjectStreamEntries(input: {
	baseEntries: readonly StreamEntry[];
	extraProjectActivity: readonly ActivityEntry[];
	projectLabel: string;
	lockedProjectId: string | undefined;
	workPhase: FeedPhase;
	inboxPhase: FeedPhase;
	actionableWorkRefs: ReadonlySet<string>;
	openInboxWorkRefs: ReadonlySet<string>;
}): StreamEntry[] {
	if (input.extraProjectActivity.length === 0) return [...input.baseEntries];
	const seen: Record<string, true> = {};
	for (const e of input.baseEntries) seen[`${e.at}|${e.text}`] = true;
	const extras: StreamEntry[] = [];
	for (const entry of input.extraProjectActivity) {
		const key = `${entry.at}|${entry.text}`;
		if (Object.hasOwn(seen, key)) continue;
		seen[key] = true;
		const needsYou = projectEntryNeedsYou(
			entry,
			input.workPhase,
			input.inboxPhase,
			input.actionableWorkRefs,
			input.openInboxWorkRefs
		);
		const tone = toneFromProjectEntry(entry, needsYou);
		extras.push({
			id: `proj-stream-${entry.id}`,
			kind: 'work',
			kindLabel: entry.kind || 'Work',
			actorName: entry.actor || 'Team',
			actorInitials: initialsOf(entry.actor || 'Team'),
			actorKind: 'person',
			text: entry.text,
			projects: input.projectLabel ? [input.projectLabel] : [],
			at: entry.at,
			tone: tone || 'accent',
			href: input.lockedProjectId
				? `/projects/${encodeURIComponent(input.lockedProjectId)}/work`
				: '/work',
			needsYou
		});
	}
	return [...input.baseEntries, ...extras].sort(
		(a, b) => (Date.parse(b.at) || 0) - (Date.parse(a.at) || 0)
	);
}
