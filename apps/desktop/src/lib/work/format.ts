import pkg from '../../../package.json';

/** "8 Nov": a short date for lists and pages (no year, no time). */
export const shortDate = (iso: string): string => new Date(iso).toLocaleDateString('en-GB', { day: 'numeric', month: 'short' });

/** What a bug report's Environment starts with: where the office runs and which app, never the browser's user agent. */
export const environmentLine = (): string => `This Mac (HQ) · Agentic Org ${pkg.version}`;
