import type { ActivityKind, ActivityRow } from '$lib/modules/activity/types';
import type { InboxItem } from '$lib/modules/inbox/types';
import type { OrgSnapshot, ProjectSummary, WorkItemSummary } from '$lib/proto';

export type AssemblyStage = 'queue' | 'agents' | 'ci' | 'review' | 'merged';
export type StreamFilterTab = 'all' | 'needs_you' | ActivityKind;
export type FeedPhase = 'loading' | 'ready' | 'error';
export type ThroughputSource =
	'events' | 'events_capped' | 'work_updates' | 'loading' | 'unavailable';

export interface AssemblyChip {
	id: string;
	shortId: string;
	title: string;
	stage: AssemblyStage;
	href: string;
	agentFirstName?: string;
	inboxId?: string;
	tooltip: string;
}

export interface AssemblyLane {
	id: string;
	name: string;
	subtitle: string;
	stages: Record<AssemblyStage, AssemblyChip[]>;
	doneCount: number;
	totalCount: number;
	pct: number;
	isComplete: boolean;
}

export interface AssemblyMilestone {
	id: string;
	name: string;
	key?: string;
	summary: string;
	doneCount: number;
	totalCount: number;
	pct: number;
	isComplete: boolean;
	lanes: AssemblyLane[];
}

export interface BottleneckSummary {
	totalOpen: number;
	agentsPct: number;
	ciPct: number;
	reviewPct: number;
	queuePct: number;
	legend: string;
	note: string;
}

export interface ReleaseGateRow {
	id: string;
	label: string;
	statusLabel: string;
	tone: 'ok' | 'warn' | 'bad' | 'neutral';
	count: number;
	href: string;
}

export interface ThroughputDay {
	key: string;
	label: string;
	count: number;
	heightPct: number;
	isToday: boolean;
}

export interface RailAgentRow {
	id: string;
	name: string;
	initials: string;
	title: string;
	status: 'building' | 'waiting' | 'paused' | 'idle';
	statusLabel: string;
	currentWork?: {
		shortId: string;
		title: string;
		href: string;
	};
	href: string;
}

export interface StreamEntry {
	id: string;
	kind: ActivityKind;
	kindLabel: string;
	actorName: string;
	actorInitials: string;
	actorKind: 'agent' | 'person';
	text: string;
	projects: string[];
	preview?: string;
	at: string;
	tone: 'ok' | 'warn' | 'bad' | 'accent' | 'sky';
	href: string;
	needsYou: boolean;
	inboxHref?: string;
	inboxLabel?: string;
	inboxTitle?: string;
	run?: {
		id: string;
		state: 'running' | 'succeeded' | 'failed';
		trigger: string;
		outcome?: string;
		log?: string[];
	};
}

export interface StreamDayGroup {
	key: string;
	label: string;
	entries: StreamEntry[];
}

export interface StationAgentSlot {
	id: string;
	name: string;
	busy: boolean;
}

export interface LineEventEntry {
	id: string;
	at: string;
	tone: 'ok' | 'warn' | 'bad' | '';
	text: string;
}

export interface ActivityCommandCenterModel {
	activeProjects: ProjectSummary[];
	selectedProject?: ProjectSummary;
	scopeTitle: string;
	scopeEyebrow: string;
	scopeSubtitle: string;
	liveStatusText: string;
	stageCounts: Record<AssemblyStage, number>;
	ciColumnSubLabel: string;
	ciStationLabel: string;
	totalItems: number;
	openItems: number;
	mergedItems: number;
	readinessPct: number;
	isComplete: boolean;
	busyAgentsCount: number;
	activeAgentsCount: number;
	needsYouCount: number;
	openInboxCount: number;
	actionableWorkCount: number;
	/** Average hours from `created_at` to `updated_at` across completed/merged items; null when none. */
	avgCycleHours: number | null;
	/** Cumulative elapsed hours waiting on human review; 0 when no items wait, null until recorded waiting spans are served. */
	waitingOnPeopleHours: number | null;
	/** Uppercase work refs/IDs currently actionable by the owner in this scope. */
	actionableWorkRefs: ReadonlySet<string>;
	/** Uppercase work refs/IDs currently referenced by open Inbox items in this scope. */
	openInboxWorkRefs: ReadonlySet<string>;
	/** Active non-former agents for the project station slot dots. */
	stationAgents: StationAgentSlot[];
	/** Items currently waiting in human review for the project dock "Needs a human" section. */
	humanWaitingItems: { id: string; title: string; href: string }[];
	/** Chronological line event log (newest first, max 30) for the project dock. */
	lineEvents: LineEventEntry[];
	milestones: AssemblyMilestone[];
	bottleneck: BottleneckSummary;
	releaseGates: ReleaseGateRow[];
	pendingApprovals: { id: string; title: string; href: string }[];
	stationsNote: string;
	throughput7d: ThroughputDay[];
	throughputSource: ThroughputSource;
	throughputMeta: string;
	throughputAriaLabel: string;
	throughputUnavailableMessage?: string;
	railAgents: RailAgentRow[];
	extraAgentsCount: number;
	streamEntries: StreamEntry[];
	streamNeedsYouCount: number;
}

export interface CommandCenterInput {
	snapshot: OrgSnapshot;
	organizationName: string;
	workItems: readonly WorkItemSummary[];
	inboxItems: readonly InboxItem[];
	activityRows: readonly ActivityRow[];
	projectScope: string;
	/** Optional accountable + contributing team IDs for project-scoped Stations roster. */
	scopedTeamIds?: readonly string[];
	ownerId?: string;
	feedPhase?: FeedPhase;
	feedMore?: number;
	workPhase?: FeedPhase;
	inboxPhase?: FeedPhase;
	nowMs?: number;
}
