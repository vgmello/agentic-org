// Mock for src/lib/modules/employees/api.ts under `bun run dev:mock`: who may hire, the HR lead's drafting, the
// dry run, provisioning step by step and the hire requests list. Data is JSON in e2e/fakes/data/employees; teams and
// people come from the org mock, and a finished hire adds the agent to it through the org's own commands, so the new
// profile, team counts and pickers show it, and show nothing before the hire completes (A97).
// Switches on the address the app was opened at:
//   ?hire=refused   the viewer may hire nowhere      ?hire=fails  provisioning stops at messaging once
//   ?hire=refreshfails  list re-reads fail       ?hire=full  Web is at its member limit       ?hire=givesup  the office gives the hire up after the record
//   ?hr=not_running|cant_start|stalls|stops|late     the HR lead can't run, never answers, stops once, or answers
//                                                    3.5 s after "still starting" shows
//   ?mock=slow|fails|empty                           loading, error and empty states (access and the list)
import { addAgent } from '$lib/modules/organization/actions';
import { orgApi } from '$lib/modules/organization/api';
import { sessionFor } from '$lib/modules/organization/session';
import type { OrgSnapshot } from '$lib/proto';
import type { EmployeesApi } from '../../../src/lib/modules/employees/api';
import type {
	HireAccess,
	HireProposal,
	HireRequest,
	HireStep,
	HireTeam,
	RuntimeCheck
} from '../../../src/lib/modules/employees/types';
import teamsData from '../data/teams/teams.json' with { type: 'json' };
import hire from '../data/employees/hire.json' with { type: 'json' };
import seed from '../data/employees/hire-requests.json' with { type: 'json' };
import {
	fail,
	gate,
	handleOf,
	officesFor,
	ownerIdOf,
	rows as memberRows,
	scenario,
	snapshotFor,
	teamRef,
	viewerId
} from './members';

const param = (name: string) => new URLSearchParams(location.search).get(name) ?? '';
const STEP_MS = 1400;
const DRAFT_MS = 2500;
const STEPS: HireStep[] = ['record', 'home', 'messaging'];
const REFUSED = "You don't have access to hire. Ask your team lead.";
const NOT_HERE = "That hire request isn't here. It may have been withdrawn.";
const maxOf = (id: string) =>
	(teamsData as Record<string, { maxMembers?: number }>)[id]?.maxMembers ?? 6;
const instanceOf = (officeName: string) =>
	(hire.instances as Record<string, string>)[officeName] ?? officeName;
const readinessOf = (profileId: string, officeName: string): RuntimeCheck =>
	((
		hire.profiles.find((p) => p.id === profileId)?.readiness as Record<string, string> | undefined
	)?.[officeName] ?? 'unknown') as RuntimeCheck;

type Stored = HireRequest & {
	/** The confirmed proposal the office provisions. */
	full?: HireProposal;
	/** When the HR lead answers (drafting) or the next provisioning step finishes (hiring). */
	nextAt?: number;
	stopsOnce?: boolean;
	failsOnce?: boolean;
	givesUp?: boolean;
	finishing?: Promise<void>;
};

let store: Stored[] | undefined;
let listReads = 0;

async function hireTeams(organizationId: string): Promise<HireTeam[]> {
	const s = await snapshotFor(organizationId);
	const members = await memberRows(organizationId);
	return s.teams.map((t) => {
		const maxMembers = maxOf(t.id);
		const count = members.filter((m) => m.homeTeam?.id === t.id && m.status !== 'inactive').length;
		return {
			...teamRef(s, t.id)!,
			parentId: t.owner_team_id,
			members: param('hire') === 'full' && t.id === 'web' ? maxMembers : count,
			maxMembers
		};
	});
}

/** Every hire call checks the caller may hire into the team, and that the team has room. */
async function check(organizationId: string, homeTeamId: string) {
	if (param('hire') === 'refused') throw fail('not_authorized', REFUSED);
	const team = (await hireTeams(organizationId)).find((t) => t.id === homeTeamId);
	if (!team) throw fail('not_authorized', REFUSED);
	if (team.members >= team.maxMembers)
		throw fail(
			'team_full',
			`${team.name} already has ${team.maxMembers} members, its limit. Raise the limit on the team's page first.`
		);
	return team;
}

const defaultOffice = async (organizationId: string) =>
	(await officesFor(organizationId))[0]?.id ?? '';

const viewerRef = (s: OrgSnapshot) => ({
	id: viewerId(),
	name: s.members.find((m) => m.id === viewerId())?.name ?? ''
});

/** The HR lead's proposal, as the mock's HR lead always writes it; reports to is the team lead, as a suggestion. */
function drafted(
	s: OrgSnapshot,
	homeTeamId: string,
	officeId: string,
	startedAt: string,
	/** A seeded draft keeps its own name and title. */
	own?: { name: string; title: string }
) {
	const d = hire.draft;
	const leadId = s.teams.find((t) => t.id === homeTeamId)?.lead_member_id;
	const lead = s.members.find((m) => m.id === leadId);
	return {
		name: own?.name ?? d.name,
		role: own?.title ?? d.title,
		drafting: { state: 'ready' as const, startedAt },
		proposal: {
			name: own?.name ?? d.name,
			handle: own ? own.name.toLowerCase() : d.handle,
			roleId: d.roleId,
			title: own?.title ?? d.title,
			focus: d.focus,
			homeTeamId,
			executionProfileId: d.executionProfileId,
			model: d.model,
			officeId
		},
		suggestedReportsTo: lead ? { id: lead.id, name: lead.name } : undefined,
		hrReason: d.hrReason
	};
}

async function load(organizationId: string): Promise<Stored[]> {
	if (store) return store;
	const s = await snapshotFor(organizationId);
	const officeId = await defaultOffice(organizationId);
	const name = (id: string) => s.members.find((m) => m.id === id)?.name ?? id;
	const now = Date.now();
	store = seed.map((r): Stored => {
		const at = new Date(now - r.agoMin * 60_000).toISOString();
		return {
			id: r.id,
			state: r.state as HireRequest['state'],
			name: r.name,
			role: r.role,
			requestedBy: { id: r.requestedBy, name: name(r.requestedBy) },
			byViewer: r.requestedBy === viewerId(),
			homeTeam: teamRef(s, r.homeTeam) ?? { id: r.homeTeam, name: r.homeTeam, ref: '' },
			createdAt: at,
			updatedAt: at,
			reason: r.reason,
			doneSteps: (r.doneSteps ?? []) as HireStep[],
			failedStep: r.failedStep as HireStep | undefined,
			closedReason: r.closedReason,
			employeeId: r.employeeId,
			full: {
				name: r.name,
				handle: r.name.toLowerCase().replace(/[^a-z0-9]+/g, '-'),
				title: r.role,
				homeTeamId: r.homeTeam,
				executionProfileId: hire.profiles[0].id,
				model: hire.profiles[0].models[0],
				officeId
			},
			...(r.state === 'drafting'
				? drafted(s, r.homeTeam, officeId, at, { name: r.name, title: r.role })
				: {})
		};
	});
	return store;
}

/** Moves a request on by the time that has passed: the HR lead answers, provisioning steps finish. */
async function advance(organizationId: string, r: Stored) {
	const now = Date.now();
	if (r.state === 'drafting' && r.drafting?.state === 'reading' && r.nextAt && now >= r.nextAt) {
		if (r.stopsOnce) {
			r.stopsOnce = false;
			r.drafting = { ...r.drafting, state: 'stopped', stoppedReason: hire.draft.stoppedReason };
		} else {
			const s = await snapshotFor(organizationId);
			const officeId = await defaultOffice(organizationId);
			Object.assign(r, drafted(s, r.homeTeam.id, officeId, r.drafting.startedAt));
		}
		r.updatedAt = new Date().toISOString();
	}
	while (r.state === 'hiring' && !r.failedStep && r.nextAt && now >= r.nextAt) {
		const step = STEPS.find((x) => !r.doneSteps.includes(x));
		if (!step) break;
		if (step === 'home' && r.givesUp) {
			Object.assign(r, {
				state: 'failed',
				closedReason: 'The office stopped and the hire was given up',
				updatedAt: new Date().toISOString()
			});
			break;
		}
		if (step === 'messaging' && r.failsOnce) {
			r.failsOnce = false;
			r.failedStep = step;
			r.updatedAt = new Date().toISOString();
			break;
		}
		r.doneSteps = [...r.doneSteps, step];
		r.nextAt += STEP_MS;
		r.updatedAt = new Date().toISOString();
	}
	if (r.state === 'hiring' && r.doneSteps.length === STEPS.length && !r.finishing)
		r.finishing = finish(organizationId, r);
	if (r.finishing) await r.finishing;
}

/** The last step done: the agent becomes a member of the org mock only now. */
async function finish(organizationId: string, r: Stored) {
	const p = r.full!;
	const profile = hire.profiles.find((x) => x.id === p.executionProfileId);
	const role = hire.roles.find((x) => x.id === p.roleId)?.name ?? p.roleText ?? p.title ?? '';
	const s = await snapshotFor(organizationId);
	const result = await addAgent(
		sessionFor(orgApi, organizationId),
		{
			name: p.name,
			messagingName: p.handle,
			runtime: profile?.driver === 'Codex' ? 'codex' : 'claude_code',
			role,
			title: p.title ?? role,
			officeId: p.officeId,
			focus: p.focus,
			model: p.model,
			runMode: 'on_call',
			teamIds: [p.homeTeamId],
			managerId: p.reportsToId
		},
		(id) => s.teams.find((t) => t.id === id)?.name ?? id
	);
	if (result.created.error) {
		r.doneSteps = [];
		r.failedStep = 'record';
		r.finishing = undefined;
		return;
	}
	r.state = 'hired';
	r.employeeId = result.memberId;
	r.updatedAt = new Date().toISOString();
}

function view(r: Stored): HireRequest {
	const shown: Partial<Stored> = { ...r, byViewer: r.requestedBy.id === viewerId() };
	delete shown.full;
	delete shown.nextAt;
	delete shown.stopsOnce;
	delete shown.failsOnce;
	delete shown.givesUp;
	delete shown.finishing;
	return structuredClone(shown) as HireRequest;
}

async function access(organizationId: string): Promise<HireAccess> {
	const s = await snapshotFor(organizationId);
	const hr = param('hr');
	return {
		teams: param('hire') === 'refused' ? [] : await hireTeams(organizationId),
		seesAll: ownerIdOf(s) === viewerId(),
		hr:
			hr === 'not_running'
				? { state: 'not_running' }
				: hr === 'cant_start'
					? { state: 'cant_start', reason: "Codex isn't signed in on hq-mac.local." }
					: { state: 'can_run' }
	};
}

async function find(organizationId: string, requestId: string) {
	const r = (await load(organizationId)).find((x) => x.id === requestId);
	if (!r) throw fail('not_found', NOT_HERE);
	if (!(await access(organizationId)).seesAll && r.requestedBy.id !== viewerId())
		throw fail('not_found', NOT_HERE);
	await advance(organizationId, r);
	return r;
}

function newRequest(s: OrgSnapshot, team: HireTeam, reason: string, id: string): Stored {
	const at = new Date().toISOString();
	return {
		id,
		state: 'drafting',
		requestedBy: viewerRef(s),
		byViewer: true,
		homeTeam: { id: team.id, name: team.name, ref: team.ref },
		createdAt: at,
		updatedAt: at,
		reason,
		doneSteps: []
	};
}

export const employeesApi: EmployeesApi = {
	async hireAccess(organizationId) {
		await gate();
		return access(organizationId);
	},
	async hireChoices(organizationId) {
		await gate();
		const s = await snapshotFor(organizationId);
		const offices = await officesFor(organizationId);
		return {
			profiles: hire.profiles.map((p) => ({
				id: p.id,
				name: p.name,
				driver: p.driver,
				models: p.models,
				readiness: Object.fromEntries(offices.map((o) => [o.id, readinessOf(p.id, o.name)]))
			})),
			offices: offices.map((o) => ({ id: o.id, name: o.name, instance: instanceOf(o.name) })),
			roles: hire.roles,
			people: s.members
				.filter((m) => m.agent?.status !== 'offboarded')
				.map((m) => ({ id: m.id, name: m.name }))
				.sort((a, b) => a.name.localeCompare(b.name)),
			takenHandles: ['hr', 'owner', ...s.members.map(handleOf)]
		};
	},
	async hireRequests(organizationId) {
		await gate();
		if (scenario() === 'empty') return [];
		// ?hire=refreshfails: the first read answers, every later one fails.
		if (param('hire') === 'refreshfails' && listReads++ > 0)
			throw fail('internal', "HQ didn't answer. Check that the office is running.");
		const all = await load(organizationId);
		const { seesAll } = await access(organizationId);
		for (const r of all) await advance(organizationId, r);
		return all
			.filter((r) => seesAll || r.requestedBy.id === viewerId())
			.toSorted((a, b) => b.createdAt.localeCompare(a.createdAt))
			.map(view);
	},
	async hireRequest(organizationId, requestId) {
		// ?hr=late: each read takes 2.5 s, so one is still out when the hirer moves on and the answer arrives.
		if (param('hr') === 'late') await new Promise((done) => setTimeout(done, 2500));
		return view(await find(organizationId, requestId));
	},
	async draft(organizationId, { reason, homeTeamId, requestId }) {
		await gate();
		const team = await check(organizationId, homeTeamId);
		const hr = param('hr');
		if (hr === 'not_running' || hr === 'cant_start')
			throw fail('cannot_start', "The HR lead can't run right now. Fill in the details yourself.");
		const all = await load(organizationId);
		const s = await snapshotFor(organizationId);
		const now = Date.now();
		let r = requestId ? all.find((x) => x.id === requestId) : undefined;
		if (requestId && r?.state !== 'drafting')
			throw fail('conflict', 'This hire request is no longer being drafted.');
		if (!r) {
			r = { ...newRequest(s, team, reason, `h${all.length + 1}-${now.toString(36)}`) };
			r.stopsOnce = hr === 'stops';
			all.push(r);
		}
		// Asking again may carry a changed description or team; nothing else from the old draft stays.
		Object.assign(r, {
			reason,
			homeTeam: { id: team.id, name: team.name, ref: team.ref },
			proposal: undefined,
			suggestedReportsTo: undefined,
			hrReason: undefined,
			updatedAt: new Date(now).toISOString()
		});
		r.drafting = {
			state: 'reading',
			startedAt: new Date(hr === 'stalls' || hr === 'late' ? now - 61_000 : now).toISOString()
		};
		r.nextAt = hr === 'stalls' ? undefined : now + (hr === 'late' ? 3500 : DRAFT_MS);
		return view(r);
	},
	async preview(organizationId, proposal) {
		await gate();
		await check(organizationId, proposal.homeTeamId);
		const s = await snapshotFor(organizationId);
		if (s.members.some((m) => handleOf(m).toLowerCase() === proposal.handle.toLowerCase()))
			throw fail('conflict', `${proposal.handle} is taken. Choose another handle.`);
		const office = (await officesFor(organizationId)).find((o) => o.id === proposal.officeId);
		if (!office) throw fail('not_found', "That office isn't in this organization any more.");
		return {
			homeFolder: `/srv/agentic-org/agents/${proposal.handle}`,
			office: { id: office.id, name: office.name, instance: instanceOf(office.name) },
			driver: hire.profiles.find((p) => p.id === proposal.executionProfileId)?.driver ?? '',
			runtime: readinessOf(proposal.executionProfileId, office.name)
		};
	},
	async confirm(organizationId, { requestId, reason, proposal }) {
		await gate();
		const team = await check(organizationId, proposal.homeTeamId);
		const all = await load(organizationId);
		const s = await snapshotFor(organizationId);
		const now = Date.now();
		let r = requestId ? all.find((x) => x.id === requestId) : undefined;
		if (requestId && r?.state !== 'drafting')
			throw fail('conflict', 'This hire request was already confirmed or withdrawn.');
		if (r && r.requestedBy.id !== viewerId())
			throw fail('not_authorized', 'Only its hirer confirms a draft.');
		if (!r) {
			r = newRequest(s, team, reason, `h${all.length + 1}-${now.toString(36)}`);
			all.push(r);
		}
		const roleName = hire.roles.find((x) => x.id === proposal.roleId)?.name;
		Object.assign(r, {
			state: 'hiring',
			name: proposal.name,
			role: proposal.title ?? roleName ?? proposal.roleText,
			homeTeam: { id: team.id, name: team.name, ref: team.ref },
			reason,
			proposal: { ...proposal },
			full: { ...proposal },
			drafting: undefined,
			doneSteps: [],
			failedStep: undefined,
			nextAt: now + STEP_MS,
			failsOnce: param('hire') === 'fails',
			givesUp: param('hire') === 'givesup',
			updatedAt: new Date(now).toISOString()
		});
		return view(r);
	},
	async withdraw(organizationId, requestId) {
		await gate();
		const r = await find(organizationId, requestId);
		if (r.requestedBy.id !== viewerId())
			throw fail('not_authorized', 'Only its hirer withdraws a draft.');
		if (r.state !== 'drafting')
			throw fail('conflict', 'Only a request that is still drafting can be withdrawn.');
		const s = await snapshotFor(organizationId);
		Object.assign(r, {
			state: 'withdrawn',
			drafting: undefined,
			closedReason: `Withdrawn by ${viewerRef(s).name || 'you'}`,
			updatedAt: new Date().toISOString()
		});
	},
	async retry(organizationId, requestId) {
		await gate();
		const r = await find(organizationId, requestId);
		if (r.state !== 'hiring' || !r.failedStep)
			throw fail('conflict', 'This hire has nothing left to finish.');
		// The permission is checked again at Try again; a team that filled up since doesn't undo a confirmed hire.
		await check(organizationId, r.homeTeam.id).catch((e: { code?: string }) => {
			if (e.code !== 'team_full') throw e;
		});
		// Try again clears the failure and finishes from the step that failed, without repeating what is done.
		Object.assign(r, {
			failedStep: undefined,
			// Slower than a step, so focus after Try again is seen before the hire settles.
			nextAt: Date.now() + 3 * STEP_MS,
			updatedAt: new Date().toISOString()
		});
	}
};
