import { mount } from 'svelte';
import Fixture from './focus-primitives.svelte';
export const mountFocusPrimitives = (target: HTMLElement) => mount(Fixture, { target });
