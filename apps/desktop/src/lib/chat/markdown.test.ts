import { describe, expect, it } from 'vitest';
import { clean, inline, markdown, plain, safeHref } from './markdown';

describe('CHAT-14 · messages render Markdown with the safety rules', () => {
	it('CHAT-14 · bold, italic, code, lists, quotes and fenced code', () => {
		const blocks = markdown('**Done** and _checked_ with `cargo test`\n\n- one\n- two\n\n> quoted\n\n```\nlet x = 1;\n```');
		expect(blocks.map((b) => b.t)).toEqual(['p', 'ul', 'quote', 'code']);
		expect(blocks[0]).toMatchObject({ inline: [{ t: 'bold' }, { t: 'text', text: ' and ' }, { t: 'italic' }, { t: 'text', text: ' with ' }, { t: 'code', text: 'cargo test' }] });
		expect(blocks[1]).toMatchObject({ items: [expect.anything(), expect.anything()] });
		expect(blocks[3]).toEqual({ t: 'code', text: 'let x = 1;' });
	});
	it('CHAT-14 · item numbers become chips', () => {
		const wanted = ['ac-T-17', 'ac-B-15', 'chk-T-12', 'chk-B-7', 'REQ-14', 'AREA-7', 'ADR-0004', 'FEAT-0007', 'DEBT-0012', 'T-12', 'B-3'];
		const refs = inline(`See ${wanted.join(', ')} but not ADR-4, T12, xT-12 or FEAT-7.`).filter((x) => x.t === 'chip');
		expect(refs).toEqual(wanted.map((ref) => ({ t: 'chip', ref })));
		expect(inline('(ac-T-17) and ac-T-17.').filter((x) => x.t === 'chip')).toHaveLength(2);
	});
	it('CHAT-14 · mentions are found and a handle inside an address is not one', () => {
		expect(inline('hi @ada and @sam-2, mail a@b.com').filter((x) => x.t === 'mention')).toEqual([
			{ t: 'mention', handle: 'ada' },
			{ t: 'mention', handle: 'sam-2' }
		]);
	});
	it('CHAT-14 · only http and https are links', () => {
		expect(safeHref('https://example.com/a?b=1')).toBe('https://example.com/a?b=1');
		for (const bad of ['javascript:alert(1)', 'data:text/html,<b>x', 'file:///etc/passwd', 'vbscript:x', '//evil.com', 'not a url']) expect(safeHref(bad)).toBeUndefined();
		const x = inline('[click](javascript:alert(1)) and [ok](https://example.com) and https://example.org/x.');
		expect(x.filter((i) => i.t === 'link')).toEqual([
			{ t: 'link', href: 'https://example.com/', text: 'ok' },
			{ t: 'link', href: 'https://example.org/x', text: 'https://example.org/x' }
		]);
		expect(plain('[click](javascript:alert(1))')).toContain('click');
	});
	it('CHAT-14 · HTML is text, never markup', () => {
		const blocks = markdown('<img src=x onerror=alert(1)> <script>alert(1)</script>');
		expect(JSON.stringify(blocks)).toContain('<script>');
		expect(blocks.every((b) => b.t === 'p' && b.inline.every((i) => i.t === 'text'))).toBe(true);
	});
	it('CHAT-14 · control, invisible and direction-changing characters are dropped', () => {
		expect(clean('a‮b​c\u0007d﻿e\r\nf')).toBe('abcde\nf');
		expect(clean('keep\ttabs\nand lines')).toBe('keep\ttabs\nand lines');
	});
	it('CHAT-14 · a link whose text is an address that is not where it goes says where it goes', () => {
		const link = (md: string) => inline(md).find((x) => x.t === 'link') as { host?: string; text: string; href: string };
		expect(link('[https://bank.com](https://evil.example/x)').host).toBe('evil.example');
		expect(link('[bank.com/login](https://evil.example)').host).toBe('evil.example');
		expect(link('[the docs](https://example.com)').host).toBeUndefined();
		expect(link('[example.com](https://www.example.com/a)').host).toBeUndefined();
		expect(link('[https://www.example.com](https://example.com)').host).toBeUndefined();
	});
	it('CHAT-14 · deep nesting and very long lines do not hang', () => {
		const slow = (text: string) => {
			const start = performance.now();
			markdown(text);
			plain(text);
			return performance.now() - start;
		};
		// Each is the whole 256 KiB a message may hold, on one line: unclosed openers, the worst case for a scanner.
		const big = 256 * 1024;
		for (const piece of ['[', '**', '*a', '_a', '`', '[a](', '[a](http://x', 'http://', '@a', 'ac-T-', '[a]('.repeat(1) + 'x'.repeat(50)]) expect(slow(piece.repeat(Math.ceil(big / piece.length)))).toBeLessThan(2000);
		expect(inline('**_**_**_'.repeat(2000)).length).toBeGreaterThan(0);
	});
});
