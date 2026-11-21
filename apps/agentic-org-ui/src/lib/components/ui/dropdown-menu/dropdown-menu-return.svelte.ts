import { getContext, setContext } from 'svelte';

const KEY = Symbol('dropdown-menu-return');

/** What a menu needs to give focus back after a click outside it: its trigger, and whether the last close was such a click. */
export type MenuReturn = { trigger: HTMLElement | null; outsideClose: boolean };

/** Set by the menu's Root, read by its Trigger and Content (each menu has its own). */
export function setMenuReturn() {
	const menu = $state<MenuReturn>({ trigger: null, outsideClose: false });
	return setContext(KEY, menu);
}
export const getMenuReturn = () => getContext<MenuReturn | undefined>(KEY);
