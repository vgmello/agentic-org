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
import { officeApi, toAppError } from '$lib/office/api';
import { workApi } from '$lib/work';

export interface HomeOfficesAdapter<TOverview = unknown> {
	overview(organizationId: string): Promise<TOverview>;
	layer(base: OfficeInfo[], overview: TOverview | undefined): OfficeInfo[];
}

/**
 * Progressive, non-blocking live data loader for Executive Home.
 * Fires all domain calls concurrently, updating each slice as soon as its own promise settles
 * with monotonic sequence and cancellation guards so stale org responses never overwrite current state.
 */
export class HomeLiveState<TOverview = unknown> {
	items = $state.raw<WorkItemSummary[] | undefined>(undefined);
	inboxLoaded = $state(false);
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
		this.items = undefined;
		this.inboxLoaded = false;
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

	connect(targetOrgId: string, baseConnectionOffices: OfficeInfo[]): () => void {
		let cancelled = false;
		this.reset();
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
		this.loadError = undefined;
		const isStale = () => this.#destroyed || isCancelled() || seq !== this.#loadSeq;

		let latestBaseOffices = baseConnectionOffices;
		let latestOverview: TOverview | undefined;
		const syncOffices = () => {
			if (!isStale()) this.offices = this.#officesAdapter.layer(latestBaseOffices, latestOverview);
		};

		const workPromise = workApi.snapshot(targetOrgId).then(
			(res) => {
				if (isStale()) return;
				this.items = res.items;
				this.nowMs = Date.now();
			},
			(err) => {
				if (isStale()) return;
				const appErr = toAppError(err);
				if (
					appErr.source === 'api' &&
					(appErr.code === 'unsupported' || appErr.code === 'unavailable')
				) {
					this.items = [];
				} else {
					this.loadError = appErr.message;
				}
			}
		);

		const inboxPromise = inboxApi.list(targetOrgId).then(
			(rows) => {
				if (isStale()) return;
				this.inboxItems = rows;
				this.inboxLoaded = true;
				this.nowMs = Date.now();
			},
			() => {
				if (isStale()) return;
				this.inboxItems = [];
				this.inboxLoaded = true;
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
					if (!isStale() && cost !== undefined) this.orgCost = cost;
				},
				() => {
					if (!isStale()) this.orgCost = undefined;
				}
			);

		void costsApi.agentBudgets(targetOrgId).then(
			(budgets) => {
				if (!isStale()) this.agentBudgets = budgets;
			},
			() => {
				if (!isStale()) this.agentBudgets = undefined;
			}
		);

		void membersApi.directory(targetOrgId).then(
			(rows) => {
				if (!isStale()) this.memberRows = rows;
			},
			() => {
				if (!isStale()) this.memberRows = undefined;
			}
		);

		void projectsApi.list(targetOrgId).then(
			(res) => {
				if (!isStale()) this.projectRows = res.projects;
			},
			() => {
				if (!isStale()) this.projectRows = undefined;
			}
		);

		void homeApi.projects(targetOrgId).then(
			(rows) => {
				if (!isStale()) this.homeProjects = rows;
			},
			() => {
				if (!isStale()) this.homeProjects = undefined;
			}
		);

		void activityApi.feed(targetOrgId, { kind: 'all' }, 12).then(
			(res) => {
				if (!isStale()) this.activityRows = res.rows;
			},
			() => {
				if (!isStale()) this.activityRows = undefined;
			}
		);

		return Promise.allSettled([workPromise, inboxPromise]).then(() => undefined);
	}

	destroy(): void {
		this.#destroyed = true;
	}
}
