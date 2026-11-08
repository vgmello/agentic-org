// Message text to a small tree the UI draws as plain text nodes (CHAT-14). There is no HTML anywhere on this path: no
// string is ever handed to `{@html}`, a link is drawn only for http(s) addresses, and control, invisible and
// direction-changing characters are dropped first (the same rule as names).

export type Inline =
	| { t: 'text'; text: string }
	| { t: 'bold'; children: Inline[] }
	| { t: 'italic'; children: Inline[] }
	| { t: 'code'; text: string }
	/** `host` is set when the text looks like an address that isn't where the link goes. */
	| { t: 'link'; href: string; text: string; host?: string }
	/** An item number: `ac-T-17`, `chk-B-7`, `REQ-14`, `AREA-7`, `ADR-0004`, `FEAT-0007`, `DEBT-0012`, `T-12`, `B-3`. */
	| { t: 'chip'; ref: string }
	| { t: 'mention'; handle: string };

export type Block = { t: 'p'; inline: Inline[] } | { t: 'quote'; inline: Inline[] } | { t: 'ul'; items: Inline[][] } | { t: 'code'; text: string };

/** Control characters (but not newline or tab), zero-width and direction-changing characters, and the byte order mark. */
// eslint-disable-next-line no-control-regex
const UNSAFE = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f​-‏‪-‮⁠-⁤⁦-⁩﻿]/g;
export const clean = (text: string): string => text.replace(/\r\n?/g, '\n').replace(UNSAFE, '');

/** Only http and https are links; anything else (javascript:, data:, file:) stays text. */
export function safeHref(href: string): string | undefined {
	try {
		const url = new URL(href);
		return url.protocol === 'https:' || url.protocol === 'http:' ? url.href : undefined;
	} catch {
		return undefined;
	}
}

/** Link text that looks like an address but isn't where the link goes: say where it goes. */
function misleading(text: string, href: string): { host: string } | Record<string, never> {
	const looks = /^(?:https?:\/\/)?([a-z0-9-]+(?:\.[a-z0-9-]+)+)(?:[/:?#]|$)/i.exec(text.trim());
	if (!looks) return {};
	const host = new URL(href).hostname.toLowerCase();
	const shown = looks[1].toLowerCase();
	return shown === host || shown === host.replace(/^www\./, '') || host === shown.replace(/^www\./, '') ? {} : { host };
}

// ---- the inline scanner ------------------------------------------------------------------------------------------------
// One pass over a line, no backtracking. Every "find the closing mark" question is answered by a finder that remembers its
// last answer, so a run of openers with no closer (`[[[[…`, `**a**a…`) costs one scan in all, not one scan each. That makes
// the work linear in the line, whatever it holds. A line over MAX_LINE is not parsed at all.

/** Work done by the scanner, counted for tests: loop turns plus the characters its finders looked at. A deterministic stand-in for a clock. */
export const scanStats = { steps: 0 };

/** A line longer than this is shown as plain text: no formatting is worth a slow scan. */
export const MAX_LINE = 8192;
const MAX_SPAN = 500;
const MAX_LINK_TEXT = 200;
const MAX_HREF = 2000;
const MAX_MENTION = 64;

const CHIP = /(?:[a-z]{2,4}-[TB]-\d{1,6}|(?:REQ|AREA)-\d{1,6}|(?:ADR|FEAT|DEBT)-\d{4}|[TB]-\d{1,6})(?![\w-])/y;
const MENTION = /[a-zA-Z0-9][\w-]{0,63}/y;
const isWord = (ch: string | undefined) => ch !== undefined && /\w/.test(ch);
const isWordOrDash = (ch: string | undefined) => ch !== undefined && /[\w-]/.test(ch);

/** The next index of `find` at or after `from`, or -1. Remembers the last answer: asked again from a later start, it reuses it. */
function finder(find: (from: number) => number, limit: number): (from: number) => number {
	let lastFrom = -1;
	let lastAt = -1;
	return (from) => {
		if (lastFrom !== -1 && from >= lastFrom && (lastAt === -1 || lastAt >= from)) return lastAt;
		lastFrom = from;
		lastAt = find(from);
		scanStats.steps += (lastAt === -1 ? limit : lastAt) - from + 1;
		return lastAt;
	};
}

const WHITESPACE = /\s/g;

function scan(text: string, depth: number): Inline[] {
	const out: Inline[] = [];
	let at = 0; // where the pending plain text starts
	let i = 0;
	const nextTick = finder((f) => text.indexOf('`', f), text.length);
	const nextStar = finder((f) => text.indexOf('*', f), text.length);
	const nextUnder = finder((f) => text.indexOf('_', f), text.length);
	const nextBracket = finder((f) => text.indexOf(']', f), text.length);
	const nextParen = finder((f) => text.indexOf(')', f), text.length);
	const nextSpace = finder((f) => {
		WHITESPACE.lastIndex = f;
		return WHITESPACE.exec(text)?.index ?? -1;
	}, text.length);
	const emit = (end: number, token: Inline) => {
		if (i > at) out.push({ t: 'text', text: text.slice(at, i) });
		out.push(token);
		i = end;
		at = end;
	};
	const children = (inner: string): Inline[] => (depth < 3 ? scan(inner, depth + 1) : [{ t: 'text', text: inner }]);
	while (i < text.length) {
		scanStats.steps++;
		const ch = text[i];
		const prev = i > 0 ? text[i - 1] : undefined;
		if (ch === '`') {
			const j = nextTick(i + 1);
			if (j > i + 1 && j - i - 1 <= MAX_SPAN) {
				emit(j + 1, { t: 'code', text: text.slice(i + 1, j) });
				continue;
			}
		} else if (ch === '*') {
			if (text[i + 1] === '*') {
				const j = nextStar(i + 2);
				if (j > i + 2 && j - i - 2 <= MAX_SPAN && text[j + 1] === '*') {
					emit(j + 2, { t: 'bold', children: children(text.slice(i + 2, j)) });
					continue;
				}
			} else if (!isWord(prev) && prev !== '*') {
				const j = nextStar(i + 1);
				if (j > i + 1 && j - i - 1 <= MAX_SPAN && !isWord(text[j + 1]) && text[j + 1] !== '*') {
					emit(j + 1, { t: 'italic', children: children(text.slice(i + 1, j)) });
					continue;
				}
			}
		} else if (ch === '_') {
			if (!isWord(prev)) {
				const j = nextUnder(i + 1);
				if (j > i + 1 && j - i - 1 <= MAX_SPAN && !isWord(text[j + 1])) {
					emit(j + 1, { t: 'italic', children: children(text.slice(i + 1, j)) });
					continue;
				}
			}
		} else if (ch === '[') {
			const j = nextBracket(i + 1);
			if (j > i + 1 && j - i - 1 <= MAX_LINK_TEXT && text[j + 1] === '(') {
				const paren = nextParen(j + 2);
				const space = nextSpace(j + 2);
				const end = paren !== -1 && (space === -1 || paren < space) ? paren : -1;
				if (end > j + 2 && end - j - 2 <= MAX_HREF) {
					const label = text.slice(i + 1, j);
					const href = safeHref(text.slice(j + 2, end));
					if (href) emit(end + 1, { t: 'link', href, text: label, ...misleading(label, href) });
					else i = end + 1; // not a link we draw: it stays text, brackets and all
					continue;
				}
			}
		} else if (ch === 'h' && (text.startsWith('http://', i) || text.startsWith('https://', i))) {
			const start = i + (text[i + 4] === 's' ? 8 : 7);
			let k = start;
			while (k < text.length && k - start <= MAX_HREF && !/[\s<>()]/.test(text[k])) {
				k++;
				scanStats.steps++;
			}
			let end = k;
			while (end > start && /[.,;:!?'"]/.test(text[end - 1])) end--;
			if (end - start >= 2) {
				const raw = text.slice(i, end);
				const href = safeHref(raw);
				if (href) emit(end, { t: 'link', href, text: raw });
				else i = end;
				continue;
			}
		} else if (ch === '@') {
			if (!isWord(prev) && prev !== '@') {
				MENTION.lastIndex = i + 1;
				const m = MENTION.exec(text);
				if (m && m[0].length <= MAX_MENTION) {
					emit(i + 1 + m[0].length, { t: 'mention', handle: m[0] });
					continue;
				}
			}
		} else if (/[A-Za-z]/.test(ch) && !isWordOrDash(prev)) {
			CHIP.lastIndex = i;
			const m = CHIP.exec(text);
			if (m) {
				emit(i + m[0].length, { t: 'chip', ref: m[0] });
				continue;
			}
		}
		i++;
	}
	if (at < text.length) out.push({ t: 'text', text: text.slice(at) });
	return out;
}

/** The inline parts of some text. A line break stays a line break, and nothing spans one. */
export function inline(text: string): Inline[] {
	const out: Inline[] = [];
	text.split('\n').forEach((line, n) => {
		if (n > 0) out.push({ t: 'text', text: '\n' });
		if (line.length > MAX_LINE) out.push({ t: 'text', text: line });
		else out.push(...scan(line, 0));
	});
	return out;
}

/** Paragraphs, bullet lists, quotes and fenced code. A line break inside a paragraph stays a line break. */
export function markdown(source: string): Block[] {
	const lines = clean(source).split('\n');
	const blocks: Block[] = [];
	let para: string[] = [];
	const flush = () => {
		if (para.length) blocks.push({ t: 'p', inline: inline(para.join('\n')) });
		para = [];
	};
	for (let i = 0; i < lines.length; i++) {
		const line = lines[i];
		if (/^```/.test(line)) {
			flush();
			const code: string[] = [];
			for (i++; i < lines.length && !/^```/.test(lines[i]); i++) code.push(lines[i]);
			blocks.push({ t: 'code', text: code.join('\n') });
		} else if (/^\s*[-*]\s+/.test(line)) {
			flush();
			const items: Inline[][] = [];
			for (; i < lines.length && /^\s*[-*]\s+/.test(lines[i]); i++) items.push(inline(lines[i].replace(/^\s*[-*]\s+/, '')));
			i--;
			blocks.push({ t: 'ul', items });
		} else if (/^>\s?/.test(line)) {
			flush();
			const quote: string[] = [];
			for (; i < lines.length && /^>\s?/.test(lines[i]); i++) quote.push(lines[i].replace(/^>\s?/, ''));
			i--;
			blocks.push({ t: 'quote', inline: inline(quote.join('\n')) });
		} else if (line.trim() === '') flush();
		else para.push(line);
	}
	flush();
	return blocks;
}

const PLAIN_KEPT = 2000;
const PLAIN_MAX_CHARS = 4000;
const plainCache = new Map<string, string>();

/** The text of a message without markup, for previews and search. Short messages are remembered: search asks for every one on each keystroke. */
export function plain(source: string): string {
	const cached = plainCache.get(source);
	if (cached !== undefined) {
		// Least recently used stays: touching it moves it to the back of the eviction order.
		plainCache.delete(source);
		plainCache.set(source, cached);
		return cached;
	}
	const text = (xs: Inline[]): string => xs.map((x) => (x.t === 'bold' || x.t === 'italic' ? text(x.children) : x.t === 'chip' ? x.ref : x.t === 'mention' ? `@${x.handle}` : x.text)).join('');
	const result = markdown(source)
		.map((b) => (b.t === 'ul' ? b.items.map(text).join(' ') : b.t === 'code' ? b.text : text(b.inline)))
		.join(' ')
		.replace(/\s+/g, ' ')
		.trim();
	if (source.length <= PLAIN_MAX_CHARS) {
		plainCache.set(source, result);
		if (plainCache.size > PLAIN_KEPT) plainCache.delete(plainCache.keys().next().value as string);
	}
	return result;
}
