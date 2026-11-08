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
		const refs = inline('See ac-17, REQ-14, ADR-0004, T-12 and B-3 but not xac-17 or ADR-4 or T12.').filter((x) => x.t === 'chip');
		expect(refs).toEqual(['ac-17', 'REQ-14', 'ADR-0004', 'T-12', 'B-3'].map((ref) => ({ t: 'chip', ref })));
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
	it('CHAT-14 · deep nesting and long runs do not hang', () => {
		const t = '**'.repeat(5000) + '*'.repeat(5000) + '_'.repeat(5000) + '['.repeat(5000);
		expect(markdown(t).length).toBeGreaterThan(0);
		expect(inline('**_**_**_'.repeat(2000)).length).toBeGreaterThan(0);
	});
});
