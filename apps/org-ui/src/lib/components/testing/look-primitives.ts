import { mount } from 'svelte';
import Fixture from './look-primitives.svelte';

/** Browser-owned API fixture; never imported by the application or release build. */
export const mountPrimitives = (target: HTMLElement) => mount(Fixture, { target });
