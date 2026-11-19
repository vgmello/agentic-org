// What the Organization settings screens read and change. Shapes follow docs/design/organization-data (accounts and
// recovery: server-installation-and-access-ux §4a; prices: costs-and-insights-ux §5).

/** The recovery code's state as HQ reports it (§4a.6). The code itself never reaches the browser (RCV-1). */
export type RecoveryState =
	| { state: 'preparing' }
	| { state: 'waiting' }
	| { state: 'shown'; shownOn?: string; again?: { count: number; lastOn?: string } }
	| {
			state: 'collected';
			collectedOn?: string;
			shownOn?: string;
			again?: { count: number; lastOn?: string };
	  }
	| { state: 'failed'; reason: string }
	| { state: 'unknown' };

export type RuntimeId = 'claude_code' | 'codex' | 'antigravity';

/** One supported runtime on this organization's offices. No credential, path or token is part of it. */
export interface RuntimeSupport {
	id: RuntimeId;
	label: string;
	availability: 'ready' | 'needs_setup' | 'unknown';
	/** Where it was seen, or what is missing: "Ready on Main office". */
	note: string;
}

export interface HandOverCandidate {
	memberId: string;
	name: string;
	username: string;
}

export interface GeneralSettings {
	organization: { id: string; name: string };
	/** The organization's workspace on the HQ machine (read-only). */
	workspace: string;
	/** The name HQ reports for its machine. */
	hqMachine: string;
	/** The instance's name; `default` when it has none. */
	instance: string;
	runtimes: RuntimeSupport[];
	viewer: { isOwner: boolean; isAdministrator: boolean };
	recovery: RecoveryState;
	administrator: { username: string; person?: string; isYou: boolean };
	/** Accounts tied to a person in one of the administrator's organizations: who the role can be handed to. */
	handOverCandidates: HandOverCandidate[];
}

export interface PriceRate {
	id: string;
	provider: string;
	model: string;
	/** US dollars per million tokens, at API list prices. */
	inputPerMTok: number;
	outputPerMTok: number;
	cacheReadPerMTok?: number;
	cacheWritePerMTok?: number;
	/** `YYYY-MM-DD`: the day the rate takes effect. */
	effectiveFrom: string;
}

export type NewPriceRate = Omit<PriceRate, 'id'>;

export type CostPeriod = 'this_month' | 'last_month';

export interface CostLine {
	label: string;
	/** Integer micro-USD, or null when the amount is unpriced or unknown (shown as a dash). */
	microUsd: number | null;
	/** Build / Run where recorded; unknown stays unknown. */
	workType?: 'Build' | 'Run' | 'Unknown';
	apiEquivalent?: boolean;
}

export interface BudgetBar {
	label: string;
	spentMicroUsd: number;
	limitMicroUsd: number;
}

export interface AgentBudgets {
	memberId: string;
	name: string;
	daily?: BudgetBar;
	monthly?: BudgetBar;
}

export interface CostSummary {
	period: CostPeriod;
	totalMicroUsd: number | null;
	includesApiEquivalent: boolean;
	/** Records that are missing or unpriced: every total they touch repeats it. */
	excluded: { missing: number; unpriced: number };
	byProject: CostLine[];
	byConsumer: CostLine[];
	notOnTaskMicroUsd: number | null;
	budgets: AgentBudgets[];
	/** The zone the day and month budgets run in, with who set it. */
	budgetZone: string;
}
