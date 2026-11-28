import { untrack } from 'svelte';
import type { OfficeInfo, WorkItemSummary } from '$lib/proto';
import { activityApi } from '$lib/modules/activity/api';
import type { ActivityRow } from '$lib/modules/activity/types';
import { costsApi } from '$lib/modules/costs/api';
import type { AgentBudgets, OrganizationCost } from '$lib/modules/costs/types';
import { homeApi } from '$lib/modules/home/api';
import type { HomeProject } from '$lib/modules/home/types';
import { inboxApi } from '$lib/modules/inbox/api';
import type { InboxItem } from '$lib/modules/inbox/types';
import { membersApi } from '$lib/modules/members/api';
import type { MemberRow } from '$lib/modules/members/types';
import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
import { projectsApi } from '$lib/modules/projects/api';
import type { ProjectRow } from '$lib/modules/projects/types';
import { buildHomeProjectHealthOverlays } from '$lib/modules/home/model/projects';
import { officeApi } from '$lib/modules/offices/office-api';
import { loadShellWorkItems } from '$lib/modules/workitems/shell-summary';
import type {
	ProjectHealthOverlay,
	ProjectHealthOverlayUpdateOptions
} from '$lib/platform/workspace/project-health';
import { toAppError } from '$lib/proto';

export interface HomeOfficesAdapter<TOverview = unknown> {
	overview(organizationId: string, signal?: AbortSignal): Promise<TOverview>;
	layer(base: OfficeInfo[], overview: TOverview | undefined): OfficeInfo[];
	onProjectHealthOverlays?: (
		organizationId: string,
		overlays: Readonly<Record<string, ProjectHealthOverlay>>,
		options?: ProjectHealthOverlayUpdateOptions
	) => void;
	onClearVolatileProjectHealthCounts?: (organizationId: string) => void;
}

export interface HomeInitialSeed {
	items?: WorkItemSummary[];
	inboxItems?: InboxItem[];
	offices?: OfficeInfo[];
}

/**
 * Progressive, non-blocking live data loader for Executive Home.
 * Fires all domain calls concurrently, updating each slice as soon as its own promise settles
 * with monotonic sequence and cancellation guards so stale org responses never overwrite current state.
 */
export class HomeLiveState<TOverview = unknown> {
	loadedOrganizationId = $state<string | undefined>(undefined);
	items = $state.raw<WorkItemSummary[] | undefined>(undefined);
	workFromSeed = $state(false);
	retryingWork = $state(false);
	allowShellFallback = $state(true);
	inboxLoaded = $state(false);
	inboxSettled = $state(false);
	offices = $state.raw<OfficeInfo[]>([]);
	orgCost = $state.raw<OrganizationCost | undefined>(undefined);
	agentBudgets = $state.raw<AgentBudgets | undefined>(undefined);
	inboxItems = $state.raw<InboxItem[]>([]);
	memberRows = $state.raw<MemberRow[] | undefined>(undefined);
	projectRows = $state.raw<ProjectRow[] | undefined>(undefined);
	homeProjects = $state.raw<HomeProject[] | undefined>(undefined);
	activityRows = $state.raw<ActivityRow[] | undefined>(undefined);
	loadError = $state<string | undefined>(undefined);
	nowMs = $state(Date.now());

	#officesAdapter: HomeOfficesAdapter<TOverview>;
	#loadSeq = 0;
	#destroyed = false;
	#activeAbort: AbortController | undefined;
	#activeOrgId: string | undefined;

	constructor(officesAdapter: HomeOfficesAdapter<TOverview>) {
		this.#officesAdapter = officesAdapter;
	}

	reset(): void {
		this.loadedOrganizationId = undefined;
		this.items = undefined;
		this.workFromSeed = false;
		this.retryingWork = false;
		this.allowShellFallback = true;
		this.inboxLoaded = false;
		this.inboxSettled = false;
		this.offices = [];
		this.orgCost = undefined;
		this.agentBudgets = undefined;
		this.inboxItems = [];
		this.memberRows = undefined;
		this.projectRows = undefined;
		this.homeProjects = undefined;
		this.activityRows = undefined;
		this.loadError = undefined;
	}

	connect(
		targetOrgId: string,
		baseConnectionOffices: OfficeInfo[],
		seed?: HomeInitialSeed
	): () => void {
		if (this.#destroyed) return () => {};
		let cancelled = false;
		this.reset();
		if (seed) {
			this.loadedOrganizationId = targetOrgId;
			if (seed.items !== undefined) {
				this.items = seed.items;
				this.workFromSeed = true;
			}
			if (seed.inboxItems !== undefined) {
				this.inboxItems = seed.inboxItems;
			}
			if (seed.offices !== undefined && seed.offices.length > 0) {
				this.offices = seed.offices;
			}
		}
		void this.load(targetOrgId, baseConnectionOffices, () => cancelled);
		return () => {
			cancelled = true;
			this.#activeAbort?.abort();
			this.#activeAbort = undefined;
		};
	}

	load(
		targetOrgId: string,
		baseConnectionOffices: OfficeInfo[],
		isCancelled: () => boolean = () => false
	): Promise<void> {
		if (this.#destroyed) return Promise.resolve();
		this.#activeOrgId = targetOrgId;
		this.#activeAbort?.abort();
		const controller = new AbortController();
		this.#activeAbort = controller;
		const signal = controller.signal;
		const seq = ++this.#loadSeq;
		const retryingFromError = untrack(() => this.loadError !== undefined);
		this.loadError = undefined;
		this.inboxSettled = false;
		if (retryingFromError) {
			this.items = undefined;
			this.workFromSeed = false;
			this.retryingWork = true;
			this.allowShellFallback = false;
		}
		const isStale = () =>
			this.#destroyed || isCancelled() || signal.aborted || seq !== this.#loadSeq;

		const workPromise = this.#loadWorkSlice(targetOrgId, signal, isStale);
		const inboxPromise = this.#loadInboxSlice(targetOrgId, signal, isStale);
		this.#loadOfficeSlices(targetOrgId, baseConnectionOffices, signal, isStale);
		this.#loadSecondarySlices(targetOrgId, signal, isStale);

		return Promise.allSettled([workPromise, inboxPromise]).then(() => undefined);
	}

	#loadWorkSlice(targetOrgId: string, signal: AbortSignal, isStale: () => boolean): Promise<void> {
		return loadShellWorkItems(targetOrgId, signal).then(
			(items) => {
				if (isStale()) return;
				this.loadedOrganizationId = targetOrgId;
				this.items = items;
				this.workFromSeed = false;
				this.retryingWork = false;
				this.allowShellFallback = false;
				this.nowMs = Date.now();
			},
			(err) => {
				if (isStale()) return;
				this.#applyWorkError(targetOrgId, err);
			}
		);
	}

	#applyWorkError(targetOrgId: string, err: unknown): void {
		this.loadedOrganizationId = targetOrgId;
		this.retryingWork = false;
		this.allowShellFallback = false;
		const appErr = toAppError(err);
		if (appErr.source === 'api' && appErr.code === 'unsupported') {
			this.items = [];
			this.workFromSeed = false;
			return;
		}
		if (this.workFromSeed) {
			this.items = undefined;
			this.workFromSeed = false;
		}
		this.loadError = appErr.message;
	}

	#loadInboxSlice(targetOrgId: string, signal: AbortSignal, isStale: () => boolean): Promise<void> {
		return inboxApi.list(targetOrgId, signal).then(
			(rows) => {
				if (isStale()) return;
				this.loadedOrganizationId = targetOrgId;
				this.inboxItems = rows;
				this.inboxLoaded = true;
				this.inboxSettled = true;
				this.nowMs = Date.now();
			},
			() => {
				if (isStale()) return;
				this.loadedOrganizationId = targetOrgId;
				this.inboxSettled = true;
			}
		);
	}

	#loadOfficeSlices(
		targetOrgId: string,
		baseConnectionOffices: OfficeInfo[],
		signal: AbortSignal,
		isStale: () => boolean
	): void {
		let latestBaseOffices = baseConnectionOffices;
		let latestOverview: TOverview | undefined;
		const syncOffices = () => {
			if (isStale()) return;
			this.loadedOrganizationId = targetOrgId;
			this.offices = this.#officesAdapter.layer(latestBaseOffices, latestOverview);
		};

		void officeApi.peekOrganizations(signal).then(
			(orgs) => {
				latestBaseOffices =
					orgs.find((o) => o.id === targetOrgId)?.offices ?? baseConnectionOffices;
				syncOffices();
			},
			() => undefined
		);

		void this.#officesAdapter.overview(targetOrgId, signal).then(
			(overview) => {
				latestOverview = overview;
				syncOffices();
			},
			() => syncOffices()
		);
	}

	#publishProjectHealthOverlays(
		targetOrgId: string,
		options: ProjectHealthOverlayUpdateOptions
	): void {
		if (!this.#officesAdapter.onProjectHealthOverlays) return;
		const overlays = buildHomeProjectHealthOverlays(this.projectRows, this.homeProjects);
		this.#officesAdapter.onProjectHealthOverlays(targetOrgId, overlays, options);
	}

	#loadProjectHealthSlices(targetOrgId: string, signal: AbortSignal, isStale: () => boolean): void {
		const projectsPromise = projectsApi.list(targetOrgId, signal).then(
			(res) => {
				if (isStale()) return false;
				this.loadedOrganizationId = targetOrgId;
				this.projectRows = res.projects;
				return true;
			},
			() => {
				if (isStale()) return false;
				this.loadedOrganizationId = targetOrgId;
				this.projectRows = undefined;
				return false;
			}
		);

		const homeProjectsPromise = homeApi.projects(targetOrgId, signal).then(
			(rows) => {
				if (isStale()) return false;
				this.loadedOrganizationId = targetOrgId;
				this.homeProjects = rows;
				return Array.isArray(rows);
			},
			() => {
				if (isStale()) return false;
				this.loadedOrganizationId = targetOrgId;
				this.homeProjects = undefined;
				return false;
			}
		);

		void Promise.all([projectsPromise, homeProjectsPromise]).then(([rowsOk, homeOk]) => {
			if (isStale()) return;
			this.#publishProjectHealthOverlays(targetOrgId, {
				preserveTeamIds: !rowsOk,
				preserveDueDate: !homeOk
			});
		});
	}

	#loadSecondarySlices(targetOrgId: string, signal: AbortSignal, isStale: () => boolean): void {
		const settle = <T>(promise: Promise<T>, onValue: (value: T) => void, onError: () => void) => {
			void promise.then(
				(value) => {
					if (isStale()) return;
					this.loadedOrganizationId = targetOrgId;
					onValue(value);
				},
				() => {
					if (isStale()) return;
					this.loadedOrganizationId = targetOrgId;
					onError();
				}
			);
		};

		settle(
			accountZone
				.ensure(targetOrgId)
				.catch(() => undefined)
				.then(() => {
					if (isStale()) return undefined;
					return costsApi.organizationCost(
						targetOrgId,
						{
							kind: 'this_month',
							timeZone: accountZone.zone || 'UTC'
						},
						signal
					);
				}),
			(cost) => {
				if (cost !== undefined) this.orgCost = cost;
			},
			() => {
				this.orgCost = undefined;
			}
		);

		settle(
			costsApi.agentBudgets(targetOrgId, signal),
			(budgets) => {
				this.agentBudgets = budgets;
			},
			() => {
				this.agentBudgets = undefined;
			}
		);

		settle(
			membersApi.directory(targetOrgId, signal),
			(rows) => {
				this.memberRows = rows;
			},
			() => {
				this.memberRows = undefined;
			}
		);

		this.#loadProjectHealthSlices(targetOrgId, signal, isStale);

		settle(
			activityApi.feed(targetOrgId, { kind: 'all' }, 12, signal),
			(res) => {
				this.activityRows = res.rows;
			},
			() => {
				this.activityRows = undefined;
			}
		);
	}

	destroy(): void {
		const orgId = this.#activeOrgId;
		this.#activeOrgId = undefined;
		this.#destroyed = true;
		this.#activeAbort?.abort();
		this.#activeAbort = undefined;
		if (orgId) this.#officesAdapter.onClearVolatileProjectHealthCounts?.(orgId);
	}
}
