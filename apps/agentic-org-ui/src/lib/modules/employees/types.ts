// Hiring an agent (our-org-in-the-app §7, §7.1; ADR-0029). Field names follow `hire_requests` and the employee
// contract in docs/design/organization-data/04-agent-structure.md: the proposal holds name, handle, title, focus,
// reporting line, execution profile, model and office; `role_id` names an existing role and `role_text` is kept apart.
import type { MemberRef, TeamRef } from '$lib/modules/members';

export type HireState = 'drafting' | 'hiring' | 'hired' | 'withdrawn' | 'failed';

/** The provisioning steps the office runs after the hirer confirms, in order (§7 Hiring). */
export type HireStep = 'record' | 'home' | 'messaging';

/** What a hire creates. Optional fields may stay unset (STAFF-5). */
export interface HireProposal {
	name: string;
	handle: string;
	roleId?: string;
	roleText?: string;
	/** With its level: "QA engineer (senior)". */
	title?: string;
	focus?: string;
	homeTeamId: string;
	reportsToId?: string;
	executionProfileId: string;
	model: string;
	officeId: string;
}

/** The HR lead's drafting turn for a request (§7 Suggestion). */
export interface HrDrafting {
	state: 'reading' | 'ready' | 'stopped';
	/** When the HR lead was asked; "still starting" is shown after 60 s without an answer. */
	startedAt: string;
	/** Why the run stopped, in words: "the run ended before it wrote a proposal". */
	stoppedReason?: string;
}

export interface HireRequest {
	id: string;
	state: HireState;
	/** The proposed agent's name, once there is one. */
	name?: string;
	/** The role or title shown beside the name. */
	role?: string;
	requestedBy: MemberRef;
	/** Whether the viewer started it: only its hirer withdraws a draft (§7.1). */
	byViewer: boolean;
	homeTeam: TeamRef;
	createdAt: string;
	updatedAt: string;
	/** Why the hire is needed, in the hirer's words (`reason`). */
	reason: string;
	/** While drafting with the HR lead. */
	drafting?: HrDrafting;
	/** What has been drafted so far: the HR lead's proposal, or the hirer's own once confirmed. */
	proposal?: Partial<HireProposal>;
	/** The HR lead's suggested manager: shown, never applied until the hirer confirms it. */
	suggestedReportsTo?: MemberRef;
	/** The HR lead's reasons for its proposal. Never present for a proposal the hirer filled in. */
	hrReason?: string;
	/** Provisioning steps finished so far. */
	doneSteps: HireStep[];
	/** Hiring stopped partway at this step (`failed_step`). */
	failedStep?: HireStep;
	/** Why it was withdrawn or given up (`closed_reason`). */
	closedReason?: string;
	/** The agent it produced, once hired. */
	employeeId?: string;
}

/** A team the viewer may hire into, with its member count and limit (§7.1 Team member limit). */
export interface HireTeam extends TeamRef {
	/** The team it sits under; none for a top-level team. */
	parentId?: string;
	members: number;
	/** `max_members`: 6 unless changed. */
	maxMembers: number;
}

/** Whether the HR lead can draft now (§2a): when it can't, hires are filled in by hand. */
export type HrLeadStatus =
	| { state: 'can_run' }
	/** The server doesn't run the HR lead yet. */
	| { state: 'not_running' }
	/** Its start failed: the reason, with the fix when known ("Codex isn't signed in on build-01."). */
	| { state: 'cant_start'; reason: string };

/** Where the viewer may hire (APPR-1). No teams means refused everywhere. */
export interface HireAccess {
	teams: HireTeam[];
	/** The owner sees every hire request; anyone else only its own. */
	seesAll: boolean;
	hr: HrLeadStatus;
}

/** What the office observed about a runtime on an instance (§7, STAFF-3, STAFF-6). */
export type RuntimeCheck = 'ready' | 'signed_out' | 'not_installed' | 'unknown';

export interface ExecutionProfile {
	id: string;
	name: string;
	/** The runtime it drives: "Claude Code", "Codex". */
	driver: string;
	models: string[];
	/** Per office ID, what the office last observed about this runtime there. */
	readiness: Record<string, RuntimeCheck>;
}

export interface HireOffice {
	id: string;
	name: string;
	/** The service machine or instance it runs on. */
	instance: string;
}

/** The choices a proposal is made from: separate from identity, as the review shows them (STAFF-1). */
export interface HireChoices {
	profiles: ExecutionProfile[];
	offices: HireOffice[];
	/** Existing roles, by stable ID. */
	roles: { id: string; name: string }[];
	/** Who a new agent may report to. */
	people: MemberRef[];
	/** Handles already used in the organization, and the reserved ones. */
	takenHandles: string[];
}

/** The dry run: what will be created, as the office would create it (§7 Review). */
export interface HirePreview {
	homeFolder: string;
	office: HireOffice;
	driver: string;
	runtime: RuntimeCheck;
}
