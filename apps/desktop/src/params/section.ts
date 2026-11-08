import type { ParamMatcher } from '@sveltejs/kit';
import { NAV } from '$lib/nav';

// The sections that don't have a page of their own yet.
export const match: ParamMatcher = (param) => NAV.some((item) => item.href === `/${param}` && param !== 'agents');
