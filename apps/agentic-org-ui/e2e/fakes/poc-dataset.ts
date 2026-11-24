/**
 * POC-mirror dataset (`?mock=poc`) mirroring `poc/v6.html` across canonical domain fakes so the Svelte app and
 * `poc/v6.html` can be compared side by side without modifying production builds or default Northwind/Atlas fixtures.
 */
import type { OrgSnapshot } from '../../src/lib/proto';
import type { ItemStatus, WorkEvent, WorkItem, WorkItemSummary } from '../../src/lib/proto-work';
import type { ActivityRow } from '../../src/lib/modules/activity/types';
import type {
	AgentBudgets,
	BudgetPeriodState,
	OrganizationCost,
	PeriodRequest,
	ProjectInvestment,
	UsageRecord,
	UsageTotals
} from '../../src/lib/modules/costs/types';
import type { HomeProject } from '../../src/lib/modules/home/types';
import type { InboxItem } from '../../src/lib/modules/inbox/types';
import type { MemberRow } from '../../src/lib/modules/members/types';
import type { OfficesOverview } from '../../src/lib/modules/offices/types';
import type { ProjectRow } from '../../src/lib/modules/projects/types';
import type { TeamRow } from '../../src/lib/modules/teams/types';
import { POC_BUILD_OFFICE_ID, POC_HQ_OFFICE_ID, pocBuildOffice } from './mock-mode';
import usage from './data/costs/usage.json' with { type: 'json' };

export const POC_USAGE_NOW_MS = Date.parse(usage.now);

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const agoIso = (mins: number) => new Date(Date.now() - mins * MINUTE).toISOString();
const aheadIso = (mins: number) => new Date(Date.now() + mins * MINUTE).toISOString();

export function pocOrgSnapshot(hqOfficeId = POC_HQ_OFFICE_ID): OrgSnapshot {
	return {
		position: { epoch: 1, seq: 120 },
		teams: [
			{
				id: 'leadership',
				name: 'Leadership',
				lead_member_id: 'maya',
				version: 1
			},
			{
				id: 'product',
				name: 'Product & Design',
				owner_team_id: 'leadership',
				lead_member_id: 'eden',
				version: 1
			},
			{
				id: 'platform',
				name: 'Platform (Rust)',
				owner_team_id: 'leadership',
				lead_member_id: 'rowan',
				version: 1
			},
			{
				id: 'app',
				name: 'App (Svelte)',
				owner_team_id: 'leadership',
				lead_member_id: 'jules',
				version: 1
			},
			{
				id: 'quality',
				name: 'Quality',
				owner_team_id: 'leadership',
				lead_member_id: 'tomas',
				version: 1
			}
		],
		members: [
			{ id: 'you', name: 'You', kind: 'person', owner: true, version: 1 },
			{
				id: 'tess',
				name: 'Tess',
				kind: 'agent',
				home_team_id: 'leadership',
				reports_to_member_id: 'you',
				office_id: hqOfficeId,
				agent: {
					status: 'active',
					role: 'chief_of_staff',
					title: 'Chief of Staff',
					messaging_name: 'tess',
					runtime: { other: 'builtin' },
					model: 'Chief of Staff (Built-in AI)'
				},
				version: 1
			},
			{
				id: 'maya',
				name: 'Maya Chen',
				kind: 'person',
				home_team_id: 'leadership',
				reports_to_member_id: 'you',
				version: 1
			},
			{
				id: 'ada',
				name: 'Ada',
				kind: 'agent',
				home_team_id: 'leadership',
				reports_to_member_id: 'you',
				office_id: hqOfficeId,
				agent: {
					status: 'active',
					role: 'engineering_manager',
					title: 'Engineering Manager',
					messaging_name: 'ada',
					runtime: 'claude_code',
					model: 'Claude Opus'
				},
				version: 1
			},
			{
				id: 'rowan',
				name: 'Rowan',
				kind: 'agent',
				home_team_id: 'platform',
				reports_to_member_id: 'ada',
				office_id: hqOfficeId,
				agent: {
					status: 'active',
					role: 'staff_engineer',
					title: 'Staff Engineer',
					messaging_name: 'rowan',
					runtime: 'claude_code',
					model: 'Claude Opus'
				},
				version: 1
			},
			{
				id: 'eden',
				name: 'Eden Lin',
				kind: 'agent',
				home_team_id: 'product',
				reports_to_member_id: 'you',
				office_id: hqOfficeId,
				agent: {
					status: 'active',
					role: 'product_designer',
					title: 'Product Designer',
					messaging_name: 'eden',
					runtime: 'claude_code',
					model: 'Claude Opus'
				},
				version: 1
			},
			{
				id: 'emery',
				name: 'Emery Park',
				kind: 'agent',
				home_team_id: 'product',
				reports_to_member_id: 'ada',
				office_id: hqOfficeId,
				agent: {
					status: 'active',
					role: 'product_manager',
					title: 'Product Manager',
					messaging_name: 'emery',
					runtime: 'claude_code',
					model: 'Claude Opus'
				},
				version: 1
			},
			{
				id: 'tomas',
				name: 'Tomas',
				kind: 'agent',
				home_team_id: 'quality',
				reports_to_member_id: 'ada',
				office_id: hqOfficeId,
				agent: {
					status: 'active',
					role: 'code_reviewer',
					title: 'Code Reviewer',
					messaging_name: 'tomas',
					runtime: 'claude_code',
					model: 'Claude Opus'
				},
				version: 1
			},
			{
				id: 'kai',
				name: 'Kai',
				kind: 'agent',
				home_team_id: 'platform',
				reports_to_member_id: 'ada',
				office_id: hqOfficeId,
				agent: {
					status: 'active',
					role: 'rust_developer',
					title: 'Rust Developer',
					messaging_name: 'kai',
					runtime: 'codex',
					model: 'gpt-6.1-sol'
				},
				version: 1
			},
			{
				id: 'sage',
				name: 'Sage',
				kind: 'agent',
				home_team_id: 'platform',
				reports_to_member_id: 'ada',
				office_id: hqOfficeId,
				agent: {
					status: 'active',
					role: 'rust_developer',
					title: 'Rust Developer',
					messaging_name: 'sage',
					runtime: 'codex',
					model: 'gpt-6.1-sol'
				},
				version: 1
			},
			{
				id: 'quinn',
				name: 'Quinn',
				kind: 'agent',
				home_team_id: 'platform',
				reports_to_member_id: 'ada',
				office_id: POC_BUILD_OFFICE_ID,
				agent: {
					status: 'active',
					role: 'rust_developer',
					title: 'Rust Developer',
					messaging_name: 'quinn',
					runtime: 'codex',
					model: 'gpt-6.1-sol'
				},
				version: 1
			},
			{
				id: 'jules',
				name: 'Jules',
				kind: 'agent',
				home_team_id: 'app',
				reports_to_member_id: 'ada',
				office_id: hqOfficeId,
				agent: {
					status: 'active',
					role: 'svelte_developer',
					title: 'Svelte Developer',
					messaging_name: 'jules',
					runtime: 'codex',
					model: 'gpt-6.1-sol'
				},
				version: 1
			},
			{
				id: 'avery',
				name: 'Avery',
				kind: 'agent',
				home_team_id: 'app',
				reports_to_member_id: 'ada',
				office_id: hqOfficeId,
				agent: {
					status: 'paused',
					role: 'svelte_developer',
					title: 'Svelte Developer',
					messaging_name: 'avery',
					runtime: 'codex',
					model: 'gpt-6.1-sol'
				},
				version: 1
			},
			{
				id: 'noor',
				name: 'Noor Haddad',
				kind: 'agent',
				home_team_id: 'quality',
				reports_to_member_id: 'tomas',
				agent: {
					status: 'active',
					role: 'support_engineer',
					title: 'Support Engineer',
					messaging_name: 'noor',
					runtime: 'claude_code',
					model: 'Claude Sonnet'
				},
				version: 1
			}
		],
		projects: [
			{ id: 'office-federation', name: 'Office federation', key: 'v0.9', version: 1 },
			{ id: 'agency-v1', name: 'Agentic Org v1.0', key: 'v1.0', version: 1 },
			{ id: 'look-feel', name: 'Look & feel refresh', key: 'v1.1', version: 1 },
			{ id: 'proto-agency', name: 'Early UI prototype', key: 'v0.1', archived: true, version: 1 }
		],
		project_teams: [
			{ id: 'pt-of-platform', project_id: 'office-federation', team_id: 'platform', version: 1 },
			{ id: 'pt-av-lead', project_id: 'agency-v1', team_id: 'leadership', version: 1 },
			{ id: 'pt-av-platform', project_id: 'agency-v1', team_id: 'platform', version: 1 },
			{ id: 'pt-av-app', project_id: 'agency-v1', team_id: 'app', version: 1 },
			{ id: 'pt-av-quality', project_id: 'agency-v1', team_id: 'quality', version: 1 },
			{ id: 'pt-lf-product', project_id: 'look-feel', team_id: 'product', version: 1 },
			{ id: 'pt-lf-app', project_id: 'look-feel', team_id: 'app', version: 1 },
			{ id: 'pt-pa-product', project_id: 'proto-agency', team_id: 'product', version: 1 }
		],
		areas: [
			{
				id: 'office-federation-general',
				project_id: 'office-federation',
				name: 'General',
				general: true,
				team_id: 'platform',
				version: 1
			},
			{
				id: 'agency-v1-general',
				project_id: 'agency-v1',
				name: 'General',
				general: true,
				team_id: 'leadership',
				version: 1
			},
			{
				id: 'look-feel-general',
				project_id: 'look-feel',
				name: 'General',
				general: true,
				team_id: 'product',
				version: 1
			},
			{
				id: 'proto-agency-general',
				project_id: 'proto-agency',
				name: 'General',
				general: true,
				team_id: 'product',
				version: 1
			}
		]
	};
}

function isoDaysFromToday(offsetDays: number): string {
	const d = new Date(Date.now() + offsetDays * 86_400_000);
	const yyyy = d.getUTCFullYear();
	const mm = String(d.getUTCMonth() + 1).padStart(2, '0');
	const dd = String(d.getUTCDate()).padStart(2, '0');
	return `${yyyy}-${mm}-${dd}`;
}

export function pocHomeProjects(): HomeProject[] {
	return [
		{
			id: 'office-federation',
			name: 'Office federation',
			done: 1,
			total: 5,
			open: 4,
			blocked: 1,
			due: isoDaysFromToday(3),
			lead: { id: 'rowan', name: 'Rowan' },
			members: 4
		},
		{
			id: 'agency-v1',
			name: 'Agentic Org v1.0',
			done: 8,
			total: 12,
			open: 4,
			blocked: 1,
			due: isoDaysFromToday(12),
			lead: { id: 'ada', name: 'Ada' },
			members: 8
		},
		{
			id: 'look-feel',
			name: 'Look & feel refresh',
			done: 3,
			total: 6,
			open: 3,
			blocked: 0,
			due: isoDaysFromToday(5),
			lead: { id: 'eden', name: 'Eden Lin' },
			members: 4
		}
	];
}

export function pocProjectRows(): ProjectRow[] {
	return [
		{
			id: 'office-federation',
			number: 1,
			slug: 'v0.9',
			name: 'Office federation',
			archived: false,
			ownerTeam: { id: 'platform', ref: 'TEAM-3', name: 'Platform (Rust)' },
			contributors: [],
			developmentProcess: null,
			tasks: { todo: 1, inProgress: 2, blocked: 1, done: 1, cancelled: 0 },
			bugs: { critical: 1, major: 0, minor: 0, trivial: 0, unset: 0 }
		},
		{
			id: 'agency-v1',
			number: 2,
			slug: 'v1.0',
			name: 'Agentic Org v1.0',
			archived: false,
			ownerTeam: { id: 'leadership', ref: 'TEAM-1', name: 'Leadership' },
			contributors: [
				{ id: 'platform', ref: 'TEAM-3', name: 'Platform (Rust)' },
				{ id: 'app', ref: 'TEAM-4', name: 'App (Svelte)' },
				{ id: 'quality', ref: 'TEAM-5', name: 'Quality' }
			],
			developmentProcess: null,
			tasks: { todo: 2, inProgress: 1, blocked: 1, done: 8, cancelled: 0 },
			bugs: { critical: 0, major: 0, minor: 2, trivial: 0, unset: 0 }
		},
		{
			id: 'look-feel',
			number: 3,
			slug: 'v1.1',
			name: 'Look & feel refresh',
			archived: false,
			ownerTeam: { id: 'product', ref: 'TEAM-2', name: 'Product & Design' },
			contributors: [{ id: 'app', ref: 'TEAM-4', name: 'App (Svelte)' }],
			developmentProcess: null,
			tasks: { todo: 0, inProgress: 3, blocked: 0, done: 3, cancelled: 0 },
			bugs: { critical: 0, major: 0, minor: 1, trivial: 0, unset: 0 }
		},
		{
			id: 'proto-agency',
			number: 4,
			slug: 'v0.1',
			name: 'Early UI prototype',
			archived: true,
			ownerTeam: { id: 'product', ref: 'TEAM-2', name: 'Product & Design' },
			contributors: [],
			developmentProcess: null,
			tasks: { todo: 0, inProgress: 0, blocked: 0, done: 6, cancelled: 0 },
			bugs: { critical: 0, major: 0, minor: 0, trivial: 0, unset: 0 }
		}
	];
}

export function pocInboxItems(): InboxItem[] {
	return [
		{
			id: 'in1',
			kind: 'approval',
			source: 'Release sign-off',
			title: "Approve publishing Kai's sync update to v1.0",
			who: { id: 'ada', name: 'Ada', kind: 'agent' },
			team: { id: 'platform', name: 'Platform (Rust)' },
			project: { id: 'agency-v1', name: 'Agentic Org v1.0' },
			why: "Kai's update makes office syncing cleaner and more reliable for v1.0. All 412 automated checks passed and Tomas approved review with zero open issues. High-impact release changes require your final sign-off before publishing.",
			recommendation: 'Ada recommends: approve. Tomas completed review with no open issues.',
			created_at: agoIso(12),
			expires_at: aheadIso(240),
			actions: ['approve', 'deny'],
			approve_label: "Approve Kai's sync",
			open: { path: '/work/T-121', label: 'Open T-121' },
			state: 'open'
		},
		{
			id: 'in18',
			kind: 'approval',
			source: 'Tess suggestion',
			title: "Reroute Jules's 3 queued tasks to Maya & Rowan",
			who: { id: 'tess', name: 'Tess', kind: 'agent' },
			team: { id: 'app', name: 'App (Svelte)' },
			why: 'Jules reached its monthly budget cap and 3 assigned tasks are sitting stalled in the queue. Maya and Rowan have available capacity in the same repositories with zero context-switch overhead and zero added spend.',
			recommendation:
				'Tess suggests: apply reroute. Recovers ~6h of queued wait time at $0 extra spend.',
			created_at: agoIso(4),
			actions: ['approve', 'deny'],
			approve_label: 'Apply Jules reroute',
			open: { path: '/work/BUG-84', label: 'Open BUG-84' },
			state: 'open'
		},
		{
			id: 'in5',
			kind: 'approval',
			source: 'Access request',
			title: "Allow local test access for Jules's security checks",
			who: { id: 'jules', name: 'Jules', kind: 'agent' },
			team: { id: 'app', name: 'App (Svelte)' },
			project: { id: 'agency-v1', name: 'Agentic Org v1.0' },
			why: 'Jules needs temporary permission to run security checks against the local test server only. No live customer or company data is exposed, and only the owner can grant local access permissions.',
			recommendation: 'Jules recommends: allow for testing only.',
			created_at: agoIso(180),
			expires_at: aheadIso(1440),
			actions: ['approve', 'deny'],
			approve_label: 'Allow test access',
			open: { path: '/work/T-124', label: 'Open T-124' },
			state: 'open'
		},
		{
			id: 'in2',
			kind: 'decision',
			source: 'Direction needed',
			title: 'Choose how offices coordinate shared files: timed access or fixed lock',
			who: { id: 'sage', name: 'Sage', kind: 'agent' },
			team: { id: 'platform', name: 'Platform (Rust)' },
			project: { id: 'office-federation', name: 'Office federation' },
			why: 'Work on second-office syncing is paused until you pick. Automatic timed access recovers cleanly if an office goes offline; a fixed per-office lock is a day faster to finish.',
			recommendation: 'Sage recommends timed access. Rowan agrees.',
			created_at: agoIso(25),
			actions: ['choose'],
			choices: [
				{ id: 'timed', label: 'Use timed access' },
				{ id: 'fixed', label: 'Fixed office lock' }
			],
			open: { path: '/work/T-118', label: 'Open T-118' },
			state: 'open'
		},
		{
			id: 'in16',
			kind: 'approval',
			source: 'Company standard',
			title: 'Require automated checks before every release?',
			who: { id: 'rowan', name: 'Rowan', kind: 'agent' },
			team: { id: 'platform', name: 'Platform (Rust)' },
			project: { id: 'agency-v1', name: 'Agentic Org v1.0' },
			why: 'Agentic Org v1.0 and Look & feel refresh both cut revision rates by running automated checks before handoff. Adopting this company-wide means every new project enforces the same quality bar automatically.',
			recommendation:
				'Rowan & Ada recommend: adopt standard. Keeps first-try approval rates above 80%.',
			created_at: agoIso(45),
			actions: ['approve', 'deny'],
			approve_label: 'Adopt CI standard',
			open: { path: '/work/T-121', label: 'Open T-121' },
			state: 'open'
		},
		{
			id: 'in17',
			kind: 'approval',
			source: 'Design sign-off',
			title: 'Approve final design polish for Look & feel refresh',
			who: { id: 'emery', name: 'Emery Park', kind: 'agent' },
			team: { id: 'product', name: 'Product & Design' },
			project: { id: 'look-feel', name: 'Look & feel refresh' },
			why: 'Half of the v1.1 refresh is already shipped (3 of 6 tasks done, 1 day ahead). Approving the final design polish unlocks the remaining 3 screens for the Nov 28 milestone, well within your monthly budget.',
			recommendation: 'Emery & Eden recommend: approve. All design and budget checks passed.',
			created_at: agoIso(60),
			actions: ['approve', 'deny'],
			approve_label: 'Approve design polish',
			open: { path: '/work/T-131', label: 'Open T-131' },
			state: 'open'
		},
		{
			id: 'in8',
			kind: 'approval',
			source: 'Run approval',
			title: 'Approve the A-OC1 pins',
			who: { id: 'rowan', name: 'Rowan', kind: 'agent' },
			team: { id: 'platform', name: 'Platform (Rust)' },
			project: { id: 'office-federation', name: 'Office federation' },
			why: 'Pins the office contract versions for v1.0.',
			created_at: agoIso(48 * 60),
			actions: [],
			state: 'handled',
			handled: { outcome: 'Approved', by: 'you', at: agoIso(46 * 60) }
		}
	];
}

export function pocWorkItems(): WorkItemSummary[] {
	const mk = (
		id: string,
		short_id: string,
		kind: WorkItemSummary['kind'],
		title: string,
		status: ItemStatus,
		project_id: string,
		assignee_id: string | undefined,
		extra: Partial<WorkItemSummary> = {}
	): WorkItemSummary => ({
		id,
		short_id,
		kind,
		number: Number(short_id.split('-')[1] ?? 1),
		title,
		status,
		project_id,
		area_id: `${project_id}-general`,
		assignee_id,
		created_by: assignee_id ?? 'ada',
		created_at: agoIso(180),
		updated_at: agoIso(20),
		severity_confirmed: kind === 'bug',
		fixes: [],
		fixed_by: [],
		checklist: {
			total: 3,
			done: status === 'done' || status === 'closed' ? 3 : 2,
			required_left: 0
		},
		done_anyway: false,
		owner_office_id: POC_HQ_OFFICE_ID,
		generation: 1,
		reopen_count: 0,
		version: 1,
		deleted: false,
		...extra
	});

	return [
		// Office federation (1 done / 5 total)
		mk('w-t118', 'T-118', 'task', 'Custody lock fix', 'blocked', 'office-federation', 'sage', {
			priority: 'high',
			blocked_reason: 'File-access choice waiting 25m · Build box office offline'
		}),
		mk(
			'w-t135',
			'T-135',
			'task',
			'HQ to satellite snapshot sync',
			'todo',
			'office-federation',
			'quinn',
			{
				priority: 'normal'
			}
		),
		mk(
			'w-t138',
			'T-138',
			'task',
			'Enrollment certificates for new offices',
			'in_progress',
			'office-federation',
			'rowan',
			{
				priority: 'low'
			}
		),
		mk(
			'w-bug81',
			'BUG-81',
			'bug',
			'Satellite stays Offline after the Mac wakes',
			'closed',
			'office-federation',
			'sage',
			{
				severity: 'critical'
			}
		),
		mk(
			'w-r11',
			'R-11',
			'request',
			'Invite flow for a second office',
			'scoping',
			'office-federation',
			'rowan'
		),

		// Agentic Org v1.0 (8 done / 12 total)
		mk(
			'w-t112',
			'T-112',
			'task',
			'Re-sequence BUG-70 nav slices for IA v2',
			'done',
			'agency-v1',
			'ada',
			{
				priority: 'high'
			}
		),
		mk('w-t121', 'T-121', 'task', 'Split codec 2a.3.2b', 'in_progress', 'agency-v1', 'kai', {
			priority: 'normal'
		}),
		mk('w-t124', 'T-124', 'task', 'CSP gates in the UI suite', 'blocked', 'agency-v1', 'jules', {
			priority: 'high',
			blocked_reason: 'Waiting for local test access permission'
		}),
		mk('w-t140', 'T-140', 'task', "Review Sage's lock fix 492e59e4", 'done', 'agency-v1', 'tomas', {
			priority: 'high'
		}),
		mk('w-t142', 'T-142', 'task', 'Write the v1.0 release notes', 'done', 'agency-v1', 'you', {
			priority: 'normal'
		}),
		mk(
			'w-t144',
			'T-144',
			'task',
			'Onboarding checklist for new people',
			'done',
			'agency-v1',
			'maya',
			{
				priority: 'low'
			}
		),
		mk(
			'w-t146',
			'T-146',
			'task',
			'Sign the Apple developer agreement for notarization',
			'todo',
			'agency-v1',
			'ada',
			{
				priority: 'high'
			}
		),
		mk(
			'w-bug70',
			'BUG-70',
			'bug',
			'Landing page hides what needs you',
			'closed',
			'agency-v1',
			'ada',
			{
				severity: 'major'
			}
		),
		mk(
			'w-bug86',
			'BUG-86',
			'bug',
			'Chat scrolls to top after sending',
			'closed',
			'agency-v1',
			'jules',
			{
				severity: 'major'
			}
		),
		mk(
			'w-bug88',
			'BUG-88',
			'bug',
			'Inbox loses replies when the office restarts',
			'triaged',
			'agency-v1',
			'rowan',
			{
				severity: 'minor'
			}
		),
		mk(
			'w-r9',
			'R-9',
			'request',
			"Read-only view of an agent's journal and memory",
			'approved',
			'agency-v1',
			'emery'
		),
		mk('w-r12', 'R-12', 'request', 'Export activity as CSV', 'approved', 'agency-v1', 'maya'),

		// Look & feel refresh (3 done / 6 total)
		mk(
			'w-t130',
			'T-130',
			'task',
			'Presence vocabulary across screens',
			'in_progress',
			'look-feel',
			'eden',
			{
				priority: 'normal'
			}
		),
		mk(
			'w-t131',
			'T-131',
			'task',
			'Information architecture v2 prototype',
			'in_progress',
			'look-feel',
			'emery',
			{
				priority: 'high'
			}
		),
		mk('w-t133', 'T-133', 'task', 'Restyle status pills', 'done', 'look-feel', 'avery', {
			priority: 'low'
		}),
		mk(
			'w-bug76',
			'BUG-76',
			'bug',
			'Status pill truncates letter by letter at 390 px',
			'closed',
			'look-feel',
			'avery',
			{
				severity: 'minor'
			}
		),
		mk(
			'w-bug84',
			'BUG-84',
			'bug',
			'Search placeholder fails contrast',
			'in_progress',
			'look-feel',
			'jules',
			{
				severity: 'minor'
			}
		),
		mk('w-r7', 'R-7', 'request', 'Dark-only look for v1.0', 'approved', 'look-feel', 'eden')
	];
}

export function pocMemberRows(): MemberRow[] {
	const hq = { id: POC_HQ_OFFICE_ID, name: 'HQ', observedAt: agoIso(1) };
	const sat = { id: POC_BUILD_OFFICE_ID, name: 'Build box', observedAt: agoIso(120) };
	return [
		{
			id: 'you',
			kind: 'person',
			name: 'You',
			handle: 'you',
			role: 'Owner',
			status: 'active',
			owner: true,
			canSignIn: true,
			directReports: 4,
			presence: { id: 'available' }
		},
		{
			id: 'maya',
			kind: 'person',
			name: 'Maya Chen',
			handle: 'maya',
			role: 'Operations',
			status: 'active',
			owner: false,
			canSignIn: true,
			directReports: 0,
			homeTeam: { id: 'leadership', ref: 'TEAM-1', name: 'Leadership' },
			presence: { id: 'offline', detail: 'Last seen yesterday' },
			reportsTo: { id: 'you', name: 'You' }
		},
		{
			id: 'tess',
			kind: 'agent',
			name: 'Tess',
			handle: 'tess',
			role: 'Chief of Staff',
			status: 'active',
			owner: false,
			canSignIn: false,
			directReports: 0,
			homeTeam: { id: 'leadership', ref: 'TEAM-1', name: 'Leadership' },
			office: hq,
			presence: { id: 'available', detail: 'Synthesizing executive brief' },
			reportsTo: { id: 'you', name: 'You' }
		},
		{
			id: 'ada',
			kind: 'agent',
			name: 'Ada',
			handle: 'ada',
			role: 'Engineering Manager',
			status: 'active',
			owner: false,
			canSignIn: false,
			directReports: 8,
			homeTeam: { id: 'leadership', ref: 'TEAM-1', name: 'Leadership' },
			office: hq,
			presence: { id: 'working', detail: 'Re-sequence nav slices for IA v2' },
			reportsTo: { id: 'you', name: 'You' },
			workingOn: {
				ref: 'T-112',
				title: 'Re-sequence BUG-70 nav slices for IA v2',
				project: 'Agentic Org v1.0'
			}
		},
		{
			id: 'rowan',
			kind: 'agent',
			name: 'Rowan',
			handle: 'rowan',
			role: 'Staff Engineer',
			status: 'active',
			owner: false,
			canSignIn: false,
			directReports: 0,
			homeTeam: { id: 'platform', ref: 'TEAM-3', name: 'Platform (Rust)' },
			office: hq,
			presence: { id: 'working', detail: 'Custody lock fix ruling' },
			reportsTo: { id: 'ada', name: 'Ada' },
			workingOn: {
				ref: 'T-138',
				title: 'Enrollment certificates for new offices',
				project: 'Office federation'
			}
		},
		{
			id: 'eden',
			kind: 'agent',
			name: 'Eden Lin',
			handle: 'eden',
			role: 'Product Designer',
			status: 'active',
			owner: false,
			canSignIn: false,
			directReports: 0,
			homeTeam: { id: 'product', ref: 'TEAM-2', name: 'Product & Design' },
			office: hq,
			presence: { id: 'working', detail: 'Presence vocabulary across screens' },
			reportsTo: { id: 'you', name: 'You' },
			workingOn: {
				ref: 'T-130',
				title: 'Presence vocabulary across screens',
				project: 'Look & feel refresh'
			}
		},
		{
			id: 'emery',
			kind: 'agent',
			name: 'Emery Park',
			handle: 'emery',
			role: 'Product Manager',
			status: 'active',
			owner: false,
			canSignIn: false,
			directReports: 0,
			homeTeam: { id: 'product', ref: 'TEAM-2', name: 'Product & Design' },
			office: hq,
			presence: { id: 'working', detail: 'Information architecture v2 prototype' },
			reportsTo: { id: 'ada', name: 'Ada' },
			workingOn: {
				ref: 'T-131',
				title: 'Information architecture v2 prototype',
				project: 'Look & feel refresh'
			}
		},
		{
			id: 'tomas',
			kind: 'agent',
			name: 'Tomas',
			handle: 'tomas',
			role: 'Code Reviewer',
			status: 'active',
			owner: false,
			canSignIn: false,
			directReports: 1,
			homeTeam: { id: 'quality', ref: 'TEAM-5', name: 'Quality' },
			office: hq,
			presence: { id: 'working', detail: "Review of Sage's lock fix 492e59e4" },
			reportsTo: { id: 'ada', name: 'Ada' }
		},
		{
			id: 'kai',
			kind: 'agent',
			name: 'Kai',
			handle: 'kai',
			role: 'Rust Developer',
			status: 'active',
			owner: false,
			canSignIn: false,
			directReports: 0,
			homeTeam: { id: 'platform', ref: 'TEAM-3', name: 'Platform (Rust)' },
			office: hq,
			presence: { id: 'with_you', detail: 'Codec split 2a.3.2b' },
			reportsTo: { id: 'ada', name: 'Ada' },
			workingOn: { ref: 'T-121', title: 'Split codec 2a.3.2b', project: 'Agentic Org v1.0' }
		},
		{
			id: 'sage',
			kind: 'agent',
			name: 'Sage',
			handle: 'sage',
			role: 'Rust Developer',
			status: 'active',
			owner: false,
			canSignIn: false,
			directReports: 0,
			homeTeam: { id: 'platform', ref: 'TEAM-3', name: 'Platform (Rust)' },
			office: hq,
			presence: { id: 'working', detail: 'Custody lock fix' },
			reportsTo: { id: 'ada', name: 'Ada' },
			workingOn: { ref: 'T-118', title: 'Custody lock fix', project: 'Office federation' }
		},
		{
			id: 'quinn',
			kind: 'agent',
			name: 'Quinn',
			handle: 'quinn',
			role: 'Rust Developer',
			status: 'active',
			owner: false,
			canSignIn: false,
			directReports: 0,
			homeTeam: { id: 'platform', ref: 'TEAM-3', name: 'Platform (Rust)' },
			office: sat,
			presence: { id: 'offline', detail: 'Build box office offline' },
			reportsTo: { id: 'ada', name: 'Ada' }
		},
		{
			id: 'jules',
			kind: 'agent',
			name: 'Jules',
			handle: 'jules',
			role: 'Svelte Developer',
			status: 'active',
			owner: false,
			canSignIn: false,
			directReports: 0,
			homeTeam: { id: 'app', ref: 'TEAM-4', name: 'App (Svelte)' },
			office: hq,
			presence: { id: 'offline', detail: 'Monthly budget reached' },
			reportsTo: { id: 'ada', name: 'Ada' },
			workingOn: {
				ref: 'BUG-84',
				title: 'Search placeholder fails contrast',
				project: 'Look & feel refresh'
			}
		},
		{
			id: 'avery',
			kind: 'agent',
			name: 'Avery',
			handle: 'avery',
			role: 'Svelte Developer',
			status: 'paused',
			owner: false,
			canSignIn: false,
			directReports: 0,
			homeTeam: { id: 'app', ref: 'TEAM-4', name: 'App (Svelte)' },
			office: hq,
			presence: { id: 'paused' },
			reportsTo: { id: 'ada', name: 'Ada' }
		},
		{
			id: 'noor',
			kind: 'agent',
			name: 'Noor Haddad',
			handle: 'noor',
			role: 'Support Engineer',
			status: 'active',
			owner: false,
			canSignIn: false,
			directReports: 0,
			homeTeam: { id: 'quality', ref: 'TEAM-5', name: 'Quality' },
			office: null,
			presence: { id: 'offline', detail: 'Not placed in an office' },
			reportsTo: { id: 'tomas', name: 'Tomas' }
		}
	];
}

export function pocOfficesOverview(): OfficesOverview {
	const sat = pocBuildOffice();
	return {
		satellites: [sat],
		details: {
			[sat.id]: {
				officeId: sat.id,
				where: 'mac-mini-2 (satellite)',
				lastSeenAt: agoIso(120),
				sync: { kind: 'behind', changes: 4 },
				version: '1.0.0-rc.3',
				follows: ['Office federation'],
				tasks: 1
			}
		},
		joinRequests: [],
		invitations: [],
		pendingAdds: [],
		outbox: [
			{
				id: 'out-1',
				officeId: sat.id,
				officeName: 'Build box',
				description: 'Resume Quinn task T-135 snapshot sync',
				state: 'pending',
				since: agoIso(95)
			}
		],
		placements: {
			quinn: sat.id
		},
		names: {},
		viewer: { isAdministrator: true }
	};
}

export function pocOrganizationCost(request: PeriodRequest): OrganizationCost {
	const now = new Date();
	const start = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), 1)).toISOString();
	const end = now.toISOString();
	const totals = (microUsd: number, records: number): UsageTotals => ({
		records,
		knownCostMicroUsd: microUsd,
		knownTokens: records * 24_000,
		cacheReadTokens: records * 4_000,
		tokenIncompleteRecords: 0,
		costIncompleteRecords: 0,
		unpricedRecords: 0,
		unknownCostRecords: 0,
		includesApiEquivalent: false
	});
	return {
		period: {
			kind: request.kind,
			startsAt: start,
			endsAt: end,
			timeZone: request.timeZone,
			toNow: true
		},
		totals: totals(142_000_000, 96),
		previous: {
			period: {
				kind: 'last_month',
				startsAt: new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth() - 1, 1)).toISOString(),
				endsAt: start,
				timeZone: request.timeZone,
				toNow: false
			},
			knownCostMicroUsd: 128_000_000
		},
		byProject: [
			{
				id: 'project:agency-v1',
				key: { dimension: 'project', projectId: 'agency-v1' },
				label: 'Agentic Org v1.0',
				projectId: 'agency-v1',
				totals: totals(68_000_000, 44)
			},
			{
				id: 'project:office-federation',
				key: { dimension: 'project', projectId: 'office-federation' },
				label: 'Office federation',
				projectId: 'office-federation',
				totals: totals(41_000_000, 28)
			},
			{
				id: 'project:look-feel',
				key: { dimension: 'project', projectId: 'look-feel' },
				label: 'Look & feel refresh',
				projectId: 'look-feel',
				totals: totals(33_000_000, 24)
			}
		],
		byConsumer: [
			{
				id: 'consumer:jules',
				key: { dimension: 'consumer', memberId: 'jules' },
				label: 'Jules',
				memberId: 'jules',
				totals: totals(40_000_000, 26)
			},
			{
				id: 'consumer:ada',
				key: { dimension: 'consumer', memberId: 'ada' },
				label: 'Ada',
				memberId: 'ada',
				totals: totals(32_000_000, 22)
			},
			{
				id: 'consumer:rowan',
				key: { dimension: 'consumer', memberId: 'rowan' },
				label: 'Rowan',
				memberId: 'rowan',
				totals: totals(28_000_000, 19)
			},
			{
				id: 'consumer:kai',
				key: { dimension: 'consumer', memberId: 'kai' },
				label: 'Kai',
				memberId: 'kai',
				totals: totals(24_000_000, 16)
			},
			{
				id: 'consumer:eden',
				key: { dimension: 'consumer', memberId: 'eden' },
				label: 'Eden Lin',
				memberId: 'eden',
				totals: totals(18_000_000, 13)
			}
		],
		byWorkType: [
			{
				id: 'work:build',
				key: { dimension: 'work_type', workType: 'build' },
				label: 'Build',
				totals: totals(112_000_000, 74)
			},
			{
				id: 'work:run',
				key: { dimension: 'work_type', workType: 'run' },
				label: 'Run',
				totals: totals(30_000_000, 22)
			}
		],
		coverage: { checkpointAt: agoIso(2), gaps: [] }
	};
}

export function pocAgentBudgets(): AgentBudgets {
	const now = new Date();
	const y = now.getUTCFullYear();
	const m = now.getUTCMonth();
	const startsAt = new Date(Date.UTC(y, m, 1)).toISOString();
	const endsAt = new Date(Date.UTC(y, m + 1, 1)).toISOString();
	const periodKey = `${y}-${String(m + 1).padStart(2, '0')}`;
	const mkMonthly = (spent: number, limit: number, hold?: 'held'): BudgetPeriodState => ({
		unit: 'usd',
		limit,
		spent,
		complete: true,
		warned: spent / limit >= 0.8,
		...(hold ? { hold } : {}),
		period: { key: periodKey, startsAt, endsAt }
	});
	// Total monthly budget = $180 ($180,000,000 micro-USD); spent = $142 (79%), Jules at cap ($40 / $40)
	return {
		zone: { name: 'America/New_York', source: 'owner_account' },
		agents: [
			{ memberId: 'ada', name: 'Ada', monthly: mkMonthly(32_000_000, 40_000_000) },
			{ memberId: 'eden', name: 'Eden Lin', monthly: mkMonthly(18_000_000, 30_000_000) },
			{ memberId: 'jules', name: 'Jules', monthly: mkMonthly(40_000_000, 40_000_000, 'held') },
			{ memberId: 'kai', name: 'Kai', monthly: mkMonthly(24_000_000, 35_000_000) },
			{ memberId: 'rowan', name: 'Rowan', monthly: mkMonthly(28_000_000, 35_000_000) }
		]
	};
}

export function pocProjectInvestments(request: { timeZone: string }): ProjectInvestment {
	const cost = pocOrganizationCost({ kind: 'this_month', timeZone: request.timeZone });
	return {
		scope: 'organization',
		thisMonth: cost.period,
		projects: cost.byProject
			.filter((p) => p.projectId)
			.map((p) => ({
				projectId: p.projectId!,
				toDate: p.totals,
				thisMonth: p.totals
			})),
		coverage: cost.coverage
	};
}

export function pocActivityRows(): ActivityRow[] {
	return [
		{
			id: 'a1',
			at: agoIso(12),
			actor: { id: 'kai', name: 'Kai', kind: 'agent' },
			kind: 'run',
			action: 'run',
			text: 'finished a run on T-121 Split codec 2a.3.2b',
			preview: 'Woken by Ada · 12 min · 412 Rust tests passing',
			href: { path: '/work/T-121', label: 'T-121' },
			teamIds: ['platform'],
			projectIds: ['agency-v1'],
			memberIds: ['kai'],
			run: {
				id: 'run-a1',
				memberId: 'kai',
				state: 'succeeded',
				started_at: agoIso(24),
				ended_at: agoIso(12),
				trigger: "Woken by Ada's message in #agency-v1",
				outcome: '412 workspace tests passed'
			}
		},
		{
			id: 'a2',
			at: agoIso(14),
			actor: { id: 'tomas', name: 'Tomas', kind: 'agent' },
			kind: 'work',
			action: 'task-done',
			text: 'marked the review of 0a520e5b READY',
			preview: 'Zero open issues on Kai codec split',
			href: { path: '/work/T-121', label: 'T-121' },
			teamIds: ['quality'],
			projectIds: ['agency-v1'],
			memberIds: ['tomas', 'kai']
		},
		{
			id: 'a3',
			at: agoIso(20),
			actor: { id: 'ada', name: 'Ada', kind: 'agent' },
			kind: 'decision',
			action: 'decision',
			text: 'approved splitting 2a.3.2b into two slices',
			href: { path: '/projects/agency-v1/work', label: 'Agentic Org v1.0' },
			teamIds: ['leadership'],
			projectIds: ['agency-v1'],
			memberIds: ['ada']
		},
		{
			id: 'a4',
			at: agoIso(25),
			actor: { id: 'rowan', name: 'Rowan', kind: 'agent' },
			kind: 'conversation',
			action: 'message',
			text: 'posted the custody lock ruling in #platform',
			preview: 'Timed lease recovers cleanly on disconnect; per-office lock is 1d faster.',
			href: { path: '/chat', label: '#platform' },
			teamIds: ['platform'],
			projectIds: ['office-federation'],
			memberIds: ['rowan']
		},
		{
			id: 'a5',
			at: agoIso(26),
			actor: { id: 'sage', name: 'Sage', kind: 'agent' },
			kind: 'run',
			action: 'run',
			text: 'stopped a run on T-118 Custody lock fix',
			preview: 'Waiting on owner ruling between timed access and fixed lock',
			href: { path: '/work/T-118', label: 'T-118' },
			teamIds: ['platform'],
			projectIds: ['office-federation'],
			memberIds: ['sage'],
			run: {
				id: 'run-a5',
				memberId: 'sage',
				state: 'failed',
				started_at: agoIso(32),
				ended_at: agoIso(26),
				trigger: 'Woken on schedule',
				outcome: 'Needs owner choice'
			}
		},
		{
			id: 'a13',
			at: agoIso(40),
			actor: { id: 'ada', name: 'Ada', kind: 'agent' },
			kind: 'org',
			action: 'member',
			text: 'asked to hire a QA engineer for Quality',
			teamIds: ['leadership', 'quality'],
			projectIds: ['agency-v1'],
			memberIds: ['ada', 'tomas']
		},
		{
			id: 'a6',
			at: agoIso(180),
			actor: { id: 'jules', name: 'Jules', kind: 'agent' },
			kind: 'work',
			action: 'task',
			text: 'moved T-124 CSP gates to Blocked',
			preview: 'Needs local test access permission',
			href: { path: '/work/T-124', label: 'T-124' },
			teamIds: ['app'],
			projectIds: ['agency-v1'],
			memberIds: ['jules']
		},
		{
			id: 'a9',
			at: agoIso(120),
			actor: { id: 'avery', name: 'Avery', kind: 'agent' },
			kind: 'work',
			action: 'task-done',
			text: 'marked T-133 Restyle status pills Done',
			href: { path: '/work/T-133', label: 'T-133' },
			teamIds: ['app'],
			projectIds: ['look-feel'],
			memberIds: ['avery']
		}
	];
}

export function pocTeamRows(): TeamRow[] {
	return [
		{
			id: 'leadership',
			ref: 'TEAM-1',
			name: 'Leadership',
			lead: { id: 'maya', name: 'Maya Chen' },
			people: 1,
			agents: 2,
			projects: [{ id: 'agency-v1', name: 'Agentic Org v1.0', role: 'owner' }],
			maxMembers: 6,
			status: 'active'
		},
		{
			id: 'product',
			ref: 'TEAM-2',
			name: 'Product & Design',
			parentId: 'leadership',
			lead: { id: 'eden', name: 'Eden Lin' },
			people: 0,
			agents: 2,
			projects: [{ id: 'look-feel', name: 'Look & feel refresh', role: 'owner' }],
			maxMembers: 6,
			status: 'active'
		},
		{
			id: 'platform',
			ref: 'TEAM-3',
			name: 'Platform (Rust)',
			parentId: 'leadership',
			lead: { id: 'rowan', name: 'Rowan' },
			people: 0,
			agents: 4,
			projects: [
				{ id: 'office-federation', name: 'Office federation', role: 'owner' },
				{ id: 'agency-v1', name: 'Agentic Org v1.0', role: 'contributor' }
			],
			maxMembers: 6,
			status: 'active'
		},
		{
			id: 'app',
			ref: 'TEAM-4',
			name: 'App (Svelte)',
			parentId: 'leadership',
			lead: { id: 'jules', name: 'Jules' },
			people: 0,
			agents: 2,
			projects: [
				{ id: 'agency-v1', name: 'Agentic Org v1.0', role: 'contributor' },
				{ id: 'look-feel', name: 'Look & feel refresh', role: 'contributor' }
			],
			maxMembers: 6,
			status: 'active'
		},
		{
			id: 'quality',
			ref: 'TEAM-5',
			name: 'Quality',
			parentId: 'leadership',
			lead: { id: 'tomas', name: 'Tomas' },
			people: 0,
			agents: 2,
			projects: [{ id: 'agency-v1', name: 'Agentic Org v1.0', role: 'contributor' }],
			maxMembers: 6,
			status: 'active'
		}
	];
}

export function pocWorkItemById(idOrRef: string): WorkItem | undefined {
	const needle = idOrRef.trim().toLowerCase();
	const summary = pocWorkItems().find(
		(i) => i.id.toLowerCase() === needle || i.short_id.toLowerCase() === needle
	);
	if (!summary) return undefined;
	const done =
		summary.status === 'done' || summary.status === 'closed' || summary.status === 'approved';
	const asOf = new Date().toISOString();
	const leadMs = Math.max(0, Date.parse(asOf) - Date.parse(summary.created_at));
	return {
		summary,
		backlinks_more: false,
		watchers_more: false,
		description: summary.blocked_reason
			? `${summary.title} — ${summary.blocked_reason}.`
			: `${summary.title} tracked in Agentic Org POC mirror dataset.`,
		checklist: [
			{
				id: `${summary.id}-step-1`,
				text: 'Implementation and unit checks complete',
				done: true,
				required: true,
				version: 1
			},
			{
				id: `${summary.id}-step-2`,
				text: 'Peer review verified',
				done,
				required: false,
				version: 1
			}
		],
		links: [],
		backlinks: [],
		watchers: summary.assignee_id ? [summary.assignee_id] : [],
		time: {
			lead_ms: leadMs,
			waiting_on_owner_ms: 0,
			blocked_ms: 0,
			rest_ms: leadMs,
			open: !done,
			rounds: 1,
			as_of: asOf
		}
	};
}

export function pocWorkActivity(idOrRef: string): WorkEvent[] {
	const item = pocWorkItemById(idOrRef);
	if (!item) return [];
	return [
		{
			target_id: item.summary.id,
			version: 1,
			at: item.summary.created_at,
			actor_id: item.summary.created_by,
			org_position: { epoch: 1, seq: 1 },
			change: { type: 'created' }
		}
	];
}

export type PocStoredUsageRecord = {
	id: string;
	occurredAt: string;
	consumer: { kind: 'agent'; memberId: string } | { kind: 'one_off'; label: string };
	runtime: string;
	provider: string;
	model: string;
	attribution: {
		basis: UsageRecord['attribution']['basis'];
		taskRef?: string;
		taskTitle?: string;
		projectId?: string;
		teamId?: string;
	};
	workType: 'build' | 'run' | null;
	tokens: [number, number, number, number, number] | null;
	tokenStatus: UsageRecord['tokenStatus'];
	costMicroUsd: number | null;
	costBasis: UsageRecord['costBasis'];
	costStatus: UsageRecord['costStatus'];
	priceId?: string | null;
	apiEquivalent?: boolean;
	issue?: string;
	supersedesId?: string;
	correction?: { reason: string; byName: string; at: string };
};

// Sums to $142.00 this month across Agentic Org's 5 active agents (Jules $40, Ada $32, Rowan $28, Kai $24, Eden $18),
// 3 projects (agency-v1 $64, office-federation $45, look-feel $33), and 2 work types (build $112, run $30).
// Shared by both `e2e/fakes/modules/costs.ts` and `e2e/fakes/modules/workitems.ts` so org/member cost links
// and task detail cost cards always show the exact same records and totals.
export function pocUsageRecords(nowMs = POC_USAGE_NOW_MS): PocStoredUsageRecord[] {
	const rec = (
		id: string,
		hoursAgo: number,
		memberId: string,
		runtime: string,
		provider: string,
		model: string,
		projectId: string,
		teamId: string,
		taskRef: string,
		taskTitle: string,
		workType: 'build' | 'run',
		costMicroUsd: number
	): PocStoredUsageRecord => ({
		id,
		occurredAt: new Date(nowMs - hoursAgo * 3600_000).toISOString(),
		consumer: { kind: 'agent', memberId },
		runtime,
		provider,
		model,
		attribution: { basis: 'current_task', taskRef, taskTitle, projectId, teamId },
		workType,
		tokens: [
			Math.round((costMicroUsd / 1_000_000) * 6000),
			Math.round((costMicroUsd / 1_000_000) * 1800),
			Math.round((costMicroUsd / 1_000_000) * 3200),
			0,
			0
		],
		tokenStatus: 'complete',
		costMicroUsd,
		costBasis: 'runtime_estimate',
		costStatus: 'complete'
	});

	return [
		rec(
			'poc-u-1',
			2,
			'jules',
			'codex',
			'openai',
			'gpt-6.1-sol',
			'agency-v1',
			'app',
			'T-124',
			'CSP gates in the UI suite',
			'build',
			18_000_000
		),
		rec(
			'poc-u-2',
			6,
			'jules',
			'codex',
			'openai',
			'gpt-6.1-sol',
			'agency-v1',
			'app',
			'BUG-86',
			'Chat scrolls to top after sending',
			'build',
			14_000_000
		),
		rec(
			'poc-u-3',
			14,
			'jules',
			'codex',
			'openai',
			'gpt-6.1-sol',
			'look-feel',
			'app',
			'BUG-84',
			'Search placeholder fails contrast',
			'run',
			8_000_000
		),
		rec(
			'poc-u-4',
			4,
			'ada',
			'claude_code',
			'anthropic',
			'claude-sonnet-4-5',
			'agency-v1',
			'leadership',
			'T-112',
			'Re-sequence BUG-70 nav slices for IA v2',
			'build',
			14_000_000
		),
		rec(
			'poc-u-5',
			16,
			'ada',
			'claude_code',
			'anthropic',
			'claude-sonnet-4-5',
			'office-federation',
			'platform',
			'T-118',
			'Custody lock fix',
			'build',
			10_000_000
		),
		rec(
			'poc-u-6',
			26,
			'ada',
			'claude_code',
			'anthropic',
			'claude-sonnet-4-5',
			'office-federation',
			'platform',
			'BUG-81',
			'Satellite stays Offline after the Mac wakes',
			'run',
			8_000_000
		),
		rec(
			'poc-u-7',
			10,
			'rowan',
			'codex',
			'openai',
			'gpt-6.1-sol',
			'office-federation',
			'platform',
			'T-138',
			'Enrollment certificates for new offices',
			'build',
			18_000_000
		),
		rec(
			'poc-u-8',
			30,
			'rowan',
			'codex',
			'openai',
			'gpt-6.1-sol',
			'agency-v1',
			'platform',
			'BUG-88',
			'Inbox loses replies when the office restarts',
			'run',
			10_000_000
		),
		rec(
			'poc-u-9',
			8,
			'kai',
			'codex',
			'openai',
			'gpt-6.1-sol',
			'look-feel',
			'product',
			'T-131',
			'Information architecture v2 prototype',
			'build',
			16_000_000
		),
		rec(
			'poc-u-10',
			22,
			'kai',
			'codex',
			'openai',
			'gpt-6.1-sol',
			'agency-v1',
			'platform',
			'T-121',
			'Split codec 2a.3.2b',
			'build',
			8_000_000
		),
		rec(
			'poc-u-11',
			12,
			'eden',
			'claude_code',
			'anthropic',
			'claude-sonnet-4-5',
			'look-feel',
			'product',
			'T-130',
			'Presence vocabulary across screens',
			'build',
			9_000_000
		),
		rec(
			'poc-u-12',
			20,
			'eden',
			'claude_code',
			'anthropic',
			'claude-sonnet-4-5',
			'office-federation',
			'platform',
			'T-135',
			'HQ to satellite snapshot sync',
			'build',
			5_000_000
		),
		rec(
			'poc-u-13',
			36,
			'eden',
			'claude_code',
			'anthropic',
			'claude-sonnet-4-5',
			'office-federation',
			'platform',
			'R-11',
			'Invite flow for a second office',
			'run',
			4_000_000
		)
	];
}

export const POC_HOUR = HOUR;
