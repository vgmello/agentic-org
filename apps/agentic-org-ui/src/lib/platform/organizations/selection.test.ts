import { describe, expect, it } from 'vitest';
import { recoveryReleaseCommand, shellQuote } from './command';
import { createSelection } from './selection';

function memory(initial?: string) {
	const items = new Map<string, string>(initial === undefined ? [] : [['org-selection', initial]]);
	return {
		items,
		getItem: (key: string) => items.get(key) ?? null,
		setItem: (key: string, value: string) => void items.set(key, value),
		removeItem: (key: string) => void items.delete(key)
	};
}
const alex = { instanceId: 'hq-1', accountId: 'alex' };
const blair = { instanceId: 'hq-1', accountId: 'blair' };

describe('selection', () => {
	it('persists the choice for one account on one instance', () => {
		const storage = memory();
		const selection = createSelection(() => storage);
		expect(selection.current(alex)).toBeUndefined();
		selection.select(alex, '0193a5c0-0000-7000-8000-000000000001');
		expect(createSelection(() => storage).current(alex)).toBe(
			'0193a5c0-0000-7000-8000-000000000001'
		);
		expect(selection.current(blair)).toBeUndefined();
		expect(selection.current({ ...alex, instanceId: 'hq-2' })).toBeUndefined();
	});
	it('replaces and clears one owner without touching another', () => {
		const storage = memory();
		const selection = createSelection(() => storage);
		selection.select(alex, 'org-a');
		selection.select(blair, 'org-b');
		selection.select(alex, 'org-c');
		expect(selection.current(alex)).toBe('org-c');
		selection.clear(alex);
		expect(selection.current(alex)).toBeUndefined();
		expect(selection.current(blair)).toBe('org-b');
		selection.clear(blair);
		expect(storage.items.has('org-selection')).toBe(false);
	});
	it.each([
		'not json',
		'[]',
		'null',
		'{"hq-1/alex":7}',
		'{"hq-1/alex":"../x"}',
		'{"__proto__":"a"}'
	])('reads damaged storage %j as no selection', (stored) => {
		expect(createSelection(() => memory(stored)).current(alex)).toBeUndefined();
		expect(
			createSelection(() => memory(stored)).current({ instanceId: '__proto__', accountId: '' })
		).toBeUndefined();
	});
	it('refuses an ID that is not an ID and survives storage that throws or is absent', () => {
		expect(() => createSelection(() => memory()).select(alex, 'a/b')).toThrow(
			'Invalid organization ID'
		);
		const broken = createSelection(() => ({
			getItem: () => {
				throw new Error('denied');
			},
			setItem: () => {
				throw new Error('quota');
			},
			removeItem: () => {}
		}));
		expect(() => broken.select(alex, 'org-a')).not.toThrow();
		expect(broken.current(alex)).toBeUndefined();
		const none = createSelection(() => undefined);
		none.select(alex, 'org-a');
		expect(none.current(alex)).toBeUndefined();
	});
});

describe('recovery release command', () => {
	it.each([
		['Side Project', "agentic-org recovery release --org 'Side Project'"],
		["Alex's Lab", "agentic-org recovery release --org 'Alex'\\''s Lab'"],
		[
			'$(rm -rf ~) `id` "x" \\ ; & |',
			'agentic-org recovery release --org \'$(rm -rf ~) `id` "x" \\ ; & |\''
		],
		["''", "agentic-org recovery release --org ''\\'''\\'''"],
		['-rf', "agentic-org recovery release --org '-rf'"]
	])('quotes %j', (name, command) => {
		expect(recoveryReleaseCommand(name)).toBe(command);
	});
	it('leaves no unquoted character of the name for the shell', () => {
		// A POSIX shell's reading: text inside single quotes is literal; outside, only \' may appear.
		const read = (quoted: string) => {
			let word = '';
			for (let i = 0; i < quoted.length;) {
				if (quoted[i] === "'") {
					const end = quoted.indexOf("'", i + 1);
					if (end < 0) throw new Error('unterminated quote');
					word += quoted.slice(i + 1, end);
					i = end + 1;
				} else if (quoted.startsWith("\\'", i)) {
					word += "'";
					i += 2;
				} else throw new Error(`unquoted ${quoted[i]}`);
			}
			return word;
		};
		for (const name of ["a'b;c'd", "'", "''$(x)'`y`", 'plain', "\\'", ' -- '])
			expect(read(shellQuote(name))).toBe(name);
	});
});
