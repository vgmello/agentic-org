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

// Bounded quantifiers keep the scan of one line linear: an unclosed `[` or `**` costs at most a few hundred characters.
const CHIP = '(?:[a-z]{2,4}-[TB]-\\d{1,6}|(?:REQ|AREA)-\\d{1,6}|(?:ADR|FEAT|DEBT)-\\d{4}|[TB]-\\d{1,6})';
const TOKEN = new RegExp(
	[
		'`([^`\\n]{1,500})`', // 1 code
		'\\*\\*([^*\\n]{1,500})\\*\\*', // 2 bold
		'(?<![\\w*])\\*([^*\\n]{1,500})\\*(?![\\w*])', // 3 italic
		'(?<![\\w])_([^_\\n]{1,500})_(?![\\w])', // 4 italic
		'\\[([^\\]\\n]{1,200})\\]\\(([^)\\s]{1,2000})\\)', // 5 text, 6 href
		'(https?://[^\\s<>()]{1,2000}[^\\s<>().,;:!?\'"])', // 7 bare url
		`(?<![\\w-])(${CHIP})(?![\\w-])`, // 8 chip
		'(?<![\\w@])@([a-zA-Z0-9][\\w-]{0,63})' // 9 mention
	].join('|'),
	'g'
);

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

/** A very long line is read in pieces, so no single line can make the scan slow. */
const PIECE = 4000;
export function inline(text: string, depth = 0): Inline[] {
	if (text.length > PIECE && depth === 0) return Array.from({ length: Math.ceil(text.length / PIECE) }, (_, i) => inlinePiece(text.slice(i * PIECE, (i + 1) * PIECE), 0)).flat();
	return inlinePiece(text, depth);
}

function inlinePiece(text: string, depth: number): Inline[] {
	const out: Inline[] = [];
	let at = 0;
	const push = (t: string) => t && out.push({ t: 'text', text: t });
	for (const m of text.matchAll(TOKEN)) {
		const i = m.index ?? 0;
		push(text.slice(at, i));
		at = i + m[0].length;
		if (m[1] !== undefined) out.push({ t: 'code', text: m[1] });
		else if (m[2] !== undefined) out.push({ t: 'bold', children: depth < 3 ? inline(m[2], depth + 1) : [{ t: 'text', text: m[2] }] });
		else if (m[3] !== undefined || m[4] !== undefined) {
			const body = (m[3] ?? m[4])!;
			out.push({ t: 'italic', children: depth < 3 ? inline(body, depth + 1) : [{ t: 'text', text: body }] });
		} else if (m[5] !== undefined) {
			const href = safeHref(m[6]);
			if (href) out.push({ t: 'link', href, text: m[5], ...misleading(m[5], href) });
			else push(m[0]);
		} else if (m[7] !== undefined) {
			const href = safeHref(m[7]);
			if (href) out.push({ t: 'link', href, text: m[7] });
			else push(m[0]);
		} else if (m[8] !== undefined) out.push({ t: 'chip', ref: m[8] });
		else if (m[9] !== undefined) out.push({ t: 'mention', handle: m[9] });
	}
	push(text.slice(at));
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

/** The text of a message without markup, for previews and search. */
export function plain(source: string): string {
	const text = (xs: Inline[]): string => xs.map((x) => (x.t === 'bold' || x.t === 'italic' ? text(x.children) : x.t === 'chip' ? x.ref : x.t === 'mention' ? `@${x.handle}` : x.t === 'link' ? x.text : x.text)).join('');
	return markdown(source)
		.map((b) => (b.t === 'ul' ? b.items.map(text).join(' ') : b.t === 'code' ? b.text : text(b.inline)))
		.join(' ')
		.replace(/\s+/g, ' ')
		.trim();
}
