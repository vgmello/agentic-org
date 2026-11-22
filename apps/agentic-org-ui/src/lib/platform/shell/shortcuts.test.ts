import { describe, expect, it } from 'vitest';
import {
	readSingleKeyShortcuts,
	SINGLE_KEY_SHORTCUTS_KEY,
	writeSingleKeyShortcuts
} from './shortcuts';

const memory = () => {
	const values = new Map<string, string>();
	return {
		getItem: (key: string) => values.get(key) ?? null,
		setItem: (key: string, value: string) => void values.set(key, value),
		values
	};
};
const broken = {
	getItem: () => {
		throw new Error('denied');
	},
	setItem: () => {
		throw new Error('denied');
	}
};

describe('single-key shortcuts setting', () => {
	it('is on by default and when storage is missing or unreadable', () => {
		expect(readSingleKeyShortcuts(memory())).toBe(true);
		expect(readSingleKeyShortcuts(undefined)).toBe(true);
		expect(readSingleKeyShortcuts(broken)).toBe(true);
	});

	it('persists 1 and 0 under the device-wide key', () => {
		const store = memory();
		expect(writeSingleKeyShortcuts(false, store)).toBe(true);
		expect(store.values.get(SINGLE_KEY_SHORTCUTS_KEY)).toBe('0');
		expect(readSingleKeyShortcuts(store)).toBe(false);
		writeSingleKeyShortcuts(true, store);
		expect(store.values.get(SINGLE_KEY_SHORTCUTS_KEY)).toBe('1');
		expect(readSingleKeyShortcuts(store)).toBe(true);
	});

	it('treats any value other than 0 as on, and reports a write the device refused', () => {
		const store = memory();
		store.setItem(SINGLE_KEY_SHORTCUTS_KEY, 'yes');
		expect(readSingleKeyShortcuts(store)).toBe(true);
		expect(writeSingleKeyShortcuts(false, broken)).toBe(false);
		expect(writeSingleKeyShortcuts(false, undefined)).toBe(false);
	});
});
