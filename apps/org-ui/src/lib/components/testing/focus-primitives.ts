import { mount } from 'svelte';
import Fixture from './focus-primitives.svelte';
import PointerFixture from './menu-pointer.svelte';
export const mountFocusPrimitives = (target: HTMLElement) => mount(Fixture, { target });
export const mountPointerMenuContracts = (target: HTMLElement, mode: string) => mount(PointerFixture, { target, props: { mode } });
