import type { ImportApi } from './api';
import type { Hints, HrAgent, Scan } from './types';

/**
 * A team to bring in, for running the UI in a plain browser: the five HR agents and the ten messaging names our own
 * team has. Invented paths under /Users/sample.
 */
const HR = '/Users/sample/projects/team/agents';
const SPOOL = '/Users/sample/.agency/spool';

const hr = (id: string, name: string, role: string, title: string, department: string, reportsTo: string, extra: Partial<HrAgent> = {}): HrAgent => ({
	id,
	name,
	project: 'ac',
	role,
	title,
	department,
	reports_to: reportsTo,
	model: 'inherit',
	status: 'active',
	hired_on: '2026-11-07',
	home: `projects/ac/agents/${id}`,
	home_path: `${HR}/projects/ac/agents/${id}`,
	...extra
});

export const SAMPLE_SCAN: Scan = {
	hr: {
		path: HR,
		agents: [
			hr('ac-staff-engineer-ada', 'ada', 'staff-engineer', 'Staff Engineer', 'Engineering', 'user'),
			hr('ac-product-designer-eden', 'Eden', 'product-designer', 'Principal Product Designer', 'Design', 'user'),
			hr('ac-software-engineer-river', 'River', 'software-engineer', 'Senior Backend Engineer', 'Engineering', 'ac-staff-engineer-ada'),
			hr('ac-software-engineer-remy', 'Remy', 'software-engineer', 'Senior Backend Engineer', 'Engineering', 'ac-staff-engineer-ada'),
			hr('ac-software-engineer-riley', 'Riley', 'software-engineer', 'Senior Frontend Engineer', 'Engineering', 'ac-staff-engineer-ada')
		],
		roles: ['product-designer', 'product-manager', 'qa-engineer', 'security-engineer', 'software-engineer', 'staff-engineer', 'technical-writer'],
		skipped: []
	},
	messaging: {
		path: SPOOL,
		names: [
			'ac-product-designer-eden',
			'ac-software-engineer-remy',
			'ac-software-engineer-river',
			'ac-staff-engineer-ada',
			'agy-senior-dev',
			'app-frontend-dev',
			'codex-dev',
			'reviewer',
			'server-dev',
			'sample'
		]
	}
};

const HINTS = '/Users/sample/teams.toml';

/** Our own team's shape (agents/.../dogfood/teams.toml): Engineering with App, Office, Agent tooling and Quality, and Design. */
export const SAMPLE_HINTS: Hints = {
	path: HINTS,
	teams: [
		{ key: 'engineering', name: 'Engineering', lead: 'ac-staff-engineer-ada', members: ['ac-staff-engineer-ada'] },
		{ key: 'app', name: 'App', parent: 'engineering', lead: 'app-frontend-dev', members: ['app-frontend-dev', 'ac-software-engineer-riley'] },
		{ key: 'office', name: 'Office', parent: 'engineering', lead: 'server-dev', members: ['server-dev', 'ac-software-engineer-river'] },
		{ key: 'agent-tooling', name: 'Agent tooling', parent: 'engineering', lead: 'ac-software-engineer-remy', members: ['ac-software-engineer-remy', 'codex-dev', 'agy-senior-dev'] },
		{ key: 'quality', name: 'Quality', parent: 'engineering', lead: 'reviewer', members: ['reviewer'] },
		{ key: 'design', name: 'Design', lead: 'ac-product-designer-eden', members: ['ac-product-designer-eden'] }
	]
};

export const mockImport: ImportApi = {
	async defaults() {
		return { hr: HR, messaging: SPOOL, user: 'sample' };
	},
	async scan(hrDir, spoolDir) {
		return {
			...(hrDir ? { hr: hrDir === HR ? SAMPLE_SCAN.hr : { path: hrDir, error: "That folder isn't there.", agents: [], roles: [], skipped: [] } } : {}),
			...(spoolDir ? { messaging: spoolDir === SPOOL ? SAMPLE_SCAN.messaging : { path: spoolDir, error: "That folder isn't there.", names: [] } } : {})
		};
	},
	async pickFolder() {
		return undefined;
	},
	/** The browser has no file picker: choosing team hints picks our own team's file. */
	async pickFile() {
		return HINTS;
	},
	async readHints(path) {
		return path === HINTS ? SAMPLE_HINTS : { path, error: "The file can't be read (No such file).", teams: [] };
	}
};
