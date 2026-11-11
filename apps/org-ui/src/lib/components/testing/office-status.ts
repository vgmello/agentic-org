import { mount } from 'svelte';
import Fixture from './office-status.svelte';

// Only the browser tests import this fixture; it is outside the application graph.
export const mountOfficeStatuses = (target: HTMLElement) => mount(Fixture, { target });
