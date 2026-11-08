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
	const SHAPES = ['[', '**', '*a', '_a', '`', '[a](', '[a](http://x', 'http://', '@a', 'ac-T-', '[a]', '[a](b c', '**a*', '_a_a', '[a]('];
	const sized = (shape: string, chars: number) => shape.repeat(Math.ceil(chars / shape.length)).slice(0, chars);

	it('CHAT-14 · adversarial lines under the 8 KiB cap are really scanned, and finish at once', () => {
		// Openers with no closer, the worst case for a scanner. These lines are under MAX_LINE, so scan() runs on every one.
		for (const shape of SHAPES) {
			const text = sized(shape, 8000);
			const start = performance.now();
			const parts = inline(text);
			expect(performance.now() - start, `shape ${shape}`).toBeLessThan(2000);
			expect(parts.length).toBeGreaterThan(0);
		}
		expect(inline('**_**_**_'.repeat(900)).length).toBeGreaterThan(0);
	});
	it('CHAT-14 · a 256 KiB line is plain text, and a whole message of them is quick', () => {
		const big = 256 * 1024;
		for (const shape of SHAPES) {
			const text = sized(shape, big);
			const lines = sized(shape, 8000) + '\n';
			const start = performance.now();
			markdown(text);
			plain(text);
			markdown(lines.repeat(Math.ceil(big / lines.length)));
			expect(performance.now() - start, `shape ${shape}`).toBeLessThan(5000);
		}
	});
	it('CHAT-14 · work grows with the line: four times the text takes well under sixteen times as long, for every shape', () => {
		// Linear is about 4x, quadratic about 16x. The bound sits between them with room for a noisy machine; the work is
		// repeated so it is a few milliseconds, not microseconds.
		const best = (text: string) => {
			const runs: number[] = [];
			for (let r = 0; r < 5; r++) {
				const start = performance.now();
				for (let k = 0; k < 40; k++) inline(text);
				runs.push(performance.now() - start);
			}
			return Math.min(...runs);
		};
		for (const shape of SHAPES) {
			const small = best(sized(shape, 2000));
			const large = best(sized(shape, 8000));
			expect(large, `shape ${shape}: ${small.toFixed(2)} ms for 2000 chars, ${large.toFixed(2)} ms for 8000`).toBeLessThan(small * 9 + 2);
		}
	});
	it('CHAT-14 · a line over 8 KiB is plain text, and a short line around it still renders', () => {
		const long = `**not bold** ${'x'.repeat(9000)}`;
		const blocks = markdown(`${long}\nand **bold** after`);
		expect(blocks).toHaveLength(1);
		const parts = (blocks[0] as { inline: { t: string; text?: string }[] }).inline;
		expect(parts[0]).toEqual({ t: 'text', text: long });
		expect(parts.some((p) => p.t === 'bold')).toBe(true);
	});
	it('CHAT-14 · plain() gives the same text before and after its cache evicts it', () => {
		const text = 'See **ac-T-17** and [the docs](https://example.com) @ada';
		const first = plain(text);
		expect(first).toBe('See ac-T-17 and the docs @ada');
		expect(plain(text)).toBe(first);
		expect(plain('')).toBe('');
		for (let i = 0; i < 2200; i++) plain(`message ${i}`);
		expect(plain(text)).toBe(first);
	});
	it('CHAT-14 · the scanner reads what the old patterns did: nesting, boundaries and unclosed marks', () => {
		const t = (md: string) => inline(md).map((x) => x.t);
		expect(t('**a _b_ c**')).toEqual(['bold']);
		expect(t('a*b*c')).toEqual(['text']);
		expect(t('snake_case_word')).toEqual(['text']);
		expect(t('*it* and _it_')).toEqual(['italic', 'text', 'italic']);
		expect(t('**unclosed')).toEqual(['text']);
		expect(t('`a` and ``')).toEqual(['code', 'text']);
		expect(t('[a](b c)')).toEqual(['text']);
		expect(t('see http://x.io/a, then')).toEqual(['text', 'link', 'text']);
		expect(inline('see http://x.io/a, then').find((x) => x.t === 'link')).toMatchObject({ text: 'http://x.io/a' });
		expect(t('mail me@x.com @ada')).toEqual(['text', 'mention']);
		expect(t('a\nb')).toEqual(['text', 'text', 'text']);
	});
});
