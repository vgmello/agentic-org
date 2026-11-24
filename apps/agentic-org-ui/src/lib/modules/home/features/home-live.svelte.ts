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
import { officeApi } from '$lib/modules/offices/office-api';
import { loadShellWorkItems } from '$lib/platform/shell/shell-data.svelte';
import { toAppError } from '$lib/proto';

export interface HomeOfficesAdapter<TOverview = unknown> {
	overview(organizationId: string): Promise<TOverview>;
	layer(base: OfficeInfo[], overview: TOverview | undefined): OfficeInfo[];
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
		};
	}

	load(
		targetOrgId: string,
		baseConnectionOffices: OfficeInfo[],
		isCancelled: () => boolean = () => false
	): Promise<void> {
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
		const isStale = () => this.#destroyed || isCancelled() || seq !== this.#loadSeq;

		let latestBaseOffices = baseConnectionOffices;
		let latestOverview: TOverview | undefined;
		const syncOffices = () => {
			if (!isStale()) {
				this.loadedOrganizationId = targetOrgId;
				this.offices = this.#officesAdapter.layer(latestBaseOffices, latestOverview);
			}
		};

		const workPromise = loadShellWorkItems(targetOrgId).then(
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
				this.loadedOrganizationId = targetOrgId;
				this.retryingWork = false;
				this.allowShellFallback = false;
				const appErr = toAppError(err);
				if (appErr.source === 'api' && appErr.code === 'unsupported') {
					this.items = [];
					this.workFromSeed = false;
				} else {
					if (this.workFromSeed) {
						this.items = undefined;
						this.workFromSeed = false;
					}
					this.loadError = appErr.message;
				}
			}
		);

		const inboxPromise = inboxApi.list(targetOrgId).then(
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

		void officeApi.peekOrganizations().then(
			(orgs) => {
				latestBaseOffices =
					orgs.find((o) => o.id === targetOrgId)?.offices ?? baseConnectionOffices;
				syncOffices();
			},
			() => undefined
		);

		void this.#officesAdapter.overview(targetOrgId).then(
			(overview) => {
				latestOverview = overview;
				syncOffices();
			},
			() => syncOffices()
		);

		void accountZone
			.ensure(targetOrgId)
			.catch(() => undefined)
			.then(() => {
				if (isStale()) return;
				const timeZone = accountZone.zone || 'UTC';
				return costsApi.organizationCost(targetOrgId, { kind: 'this_month', timeZone });
			})
			.then(
				(cost) => {
					if (!isStale() && cost !== undefined) {
						this.loadedOrganizationId = targetOrgId;
						this.orgCost = cost;
					}
				},
				() => {
					if (!isStale()) {
						this.loadedOrganizationId = targetOrgId;
						this.orgCost = undefined;
					}
				}
			);

		void costsApi.agentBudgets(targetOrgId).then(
			(budgets) => {
				if (!isStale()) {
					this.loadedOrganizationId = targetOrgId;
					this.agentBudgets = budgets;
				}
			},
			() => {
				if (!isStale()) {
					this.loadedOrganizationId = targetOrgId;
					this.agentBudgets = undefined;
				}
			}
		);

		void membersApi.directory(targetOrgId).then(
			(rows) => {
				if (!isStale()) {
					this.loadedOrganizationId = targetOrgId;
					this.memberRows = rows;
				}
			},
			() => {
				if (!isStale()) {
					this.loadedOrganizationId = targetOrgId;
					this.memberRows = undefined;
				}
			}
		);

		void projectsApi.list(targetOrgId).then(
			(res) => {
				if (!isStale()) {
					this.loadedOrganizationId = targetOrgId;
					this.projectRows = res.projects;
				}
			},
			() => {
				if (!isStale()) {
					this.loadedOrganizationId = targetOrgId;
					this.projectRows = undefined;
				}
			}
		);

		void homeApi.projects(targetOrgId).then(
			(rows) => {
				if (!isStale()) {
					this.loadedOrganizationId = targetOrgId;
					this.homeProjects = rows;
				}
			},
			() => {
				if (!isStale()) {
					this.loadedOrganizationId = targetOrgId;
					this.homeProjects = undefined;
				}
			}
		);

		void activityApi.feed(targetOrgId, { kind: 'all' }, 12).then(
			(res) => {
				if (!isStale()) {
					this.loadedOrganizationId = targetOrgId;
					this.activityRows = res.rows;
				}
			},
			() => {
				if (!isStale()) {
					this.loadedOrganizationId = targetOrgId;
					this.activityRows = undefined;
				}
			}
		);

		return Promise.allSettled([workPromise, inboxPromise]).then(() => undefined);
	}

	destroy(): void {
		this.#destroyed = true;
	}
}
