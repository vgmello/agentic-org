import type { ParamMatcher } from '@sveltejs/kit';
// The sections that don't have a page of their own yet.
const PLACEHOLDERS = ['projects', 'tasks', 'sessions', 'activity', 'settings'];

export const match: ParamMatcher = (param) => PLACEHOLDERS.includes(param);
