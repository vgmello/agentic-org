import type { ParamMatcher } from '@sveltejs/kit';

// The sections that don't have a page of their own yet. (Sections with their own route, like Agents, the
// Org chart and Offices, win over this one anyway; listing only the placeholders keeps that obvious.)
const PLACEHOLDERS = ['tasks', 'sessions', 'activity', 'settings'];

export const match: ParamMatcher = (param) => PLACEHOLDERS.includes(param);
