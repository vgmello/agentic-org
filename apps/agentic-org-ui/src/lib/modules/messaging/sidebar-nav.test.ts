import { describe, expect, it } from 'vitest';
import { isTextEntryTarget, nextSidebarIndex, sidebarNavAction } from './sidebar-nav';

const key = (
	keyName: string,
	overrides: Partial<{
		altKey: boolean;
		ctrlKey: boolean;
		metaKey: boolean;
		isComposing: boolean;
		keyCode: number;
	}> = {}
) => ({
	key: keyName,
	altKey: false,
	ctrlKey: false,
	metaKey: false,
	isComposing: false,
	keyCode: 0,
	...overrides
});

describe('sidebarNavAction', () => {
	it('maps ArrowDown, ArrowUp, Home, and End without modifiers', () => {
		expect(sidebarNavAction(key('ArrowDown'))).toBe('down');
		expect(sidebarNavAction(key('ArrowUp'))).toBe('up');
		expect(sidebarNavAction(key('Home'))).toBe('home');
		expect(sidebarNavAction(key('End'))).toBe('end');
		expect(sidebarNavAction(key('Enter'))).toBeUndefined();
		expect(sidebarNavAction(key('Tab'))).toBeUndefined();
	});

	it('ignores modified shortcuts and IME composition events', () => {
		expect(sidebarNavAction(key('ArrowDown', { altKey: true }))).toBeUndefined();
		expect(sidebarNavAction(key('ArrowDown', { ctrlKey: true }))).toBeUndefined();
		expect(sidebarNavAction(key('ArrowDown', { metaKey: true }))).toBeUndefined();
		expect(sidebarNavAction(key('ArrowDown', { isComposing: true }))).toBeUndefined();
		expect(sidebarNavAction(key('ArrowDown', { keyCode: 229 }))).toBeUndefined();
	});
});

describe('nextSidebarIndex', () => {
	it('moves cleanly within bounds and clamps at list edges', () => {
		expect(nextSidebarIndex(0, 5, 'down')).toBe(1);
		expect(nextSidebarIndex(4, 5, 'down')).toBe(-1);
		expect(nextSidebarIndex(2, 5, 'up')).toBe(1);
		expect(nextSidebarIndex(0, 5, 'up')).toBe(-1);
		expect(nextSidebarIndex(3, 5, 'home')).toBe(0);
		expect(nextSidebarIndex(1, 5, 'end')).toBe(4);
	});

	it('returns -1 for empty lists or out-of-range current index', () => {
		expect(nextSidebarIndex(0, 0, 'down')).toBe(-1);
		expect(nextSidebarIndex(-1, 4, 'down')).toBe(-1);
		expect(nextSidebarIndex(4, 4, 'up')).toBe(-1);
	});
});

describe('isTextEntryTarget', () => {
	it('identifies text inputs, textareas, selects, and contenteditable targets', () => {
		expect(isTextEntryTarget({ tagName: 'INPUT' } as unknown as EventTarget)).toBe(true);
		expect(isTextEntryTarget({ tagName: 'TEXTAREA' } as unknown as EventTarget)).toBe(true);
		expect(isTextEntryTarget({ tagName: 'SELECT' } as unknown as EventTarget)).toBe(true);
		expect(
			isTextEntryTarget({ tagName: 'DIV', isContentEditable: true } as unknown as EventTarget)
		).toBe(true);
		expect(isTextEntryTarget({ tagName: 'BUTTON' } as unknown as EventTarget)).toBe(false);
		expect(isTextEntryTarget(null)).toBe(false);
	});
});
