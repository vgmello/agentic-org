import { createContext, untrack } from 'svelte';
import type { ItemStatus, OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
import { inboxApi } from '$lib/modules/inbox/api';
import type { InboxItem } from '$lib/modules/inbox/types';
import { orgApi } from '$lib/modules/organization/api';
import { owner } from '$lib/modules/organization/model';
import { workApi } from '$lib/modules/work';
import { needsYou } from '$lib/modules/work/saved';
import { workItemsApi } from '$lib/modules/workitems/api';
import type { TaskRow } from '$lib/modules/workitems/types';
import { areaCapability } from './capabilities';

function summaryStatus(row: TaskRow): ItemStatus {
	if (row.type === 'bug') {
		if (row.status === 'done' || row.status === 'cancelled') return 'closed';
		if (row.awaitingVerification) return 'fixed';
		return row.status;
	}
	if (row.type === 'request') {
		if (row.status === 'done') return 'approved';
		if (row.status === 'cancelled') return 'declined';
		if (row.needsMe?.source === 'approval') return 'scoped';
		return row.status;
	}
	return row.status === 'cancelled' ? 'closed' : row.status;
}

function rowToSummary(row: TaskRow): WorkItemSummary {
	const numMatch = /\d+$/.exec(row.ref);
	return {
		id: row.id,
		short_id: row.ref,
		kind: row.type,
		number: numMatch ? Number(numMatch[0]) : 0,
		title: row.title,
		status: summaryStatus(row),
		...(row.type === 'task' && row.status === 'cancelled' ? { resolution: 'cancelled' } : {}),
		priority: row.priority,
		severity: row.severity ?? undefined,
		project_id: row.project?.id,
		assignee_id: row.assignee?.id,
		created_by: row.reporter.id,
		created_at: row.createdAt,
		updated_at: row.updatedAt,
		blocked_reason: row.blockedReason ?? undefined,
		severity_confirmed: row.type === 'bug',
		fixes: [],
		fixed_by: [],
		checklist: {
			total: row.checklist.total,
			done: row.checklist.done,
			required_left: row.checklist.requiredLeft
		},
		done_anyway: false,
		owner_office_id: '',
		generation: 1,
		reopen_count: row.reopenCount,
		version: row.revision,
		deleted: false
	};
}

export async function loadShellWorkItems(organizationId: string): Promise<WorkItemSummary[]> {
	if (areaCapability('/work').available) {
		try {
			const queue = await workItemsApi.queue(organizationId, { kind: 'organization' });
			return queue.rows.map(rowToSummary);
		} catch {
			// Fall back to legacy workApi when workItemsApi is unavailable.
		}
	}
	return (await workApi.snapshot(organizationId)).items;
}

export interface ShellRefreshResult {
	inboxUpdated: boolean;
	workUpdated: boolean;
}

export interface ShellDataOptions {
	getOrganizationId: () => string;
	getBaseOffices: () => OfficeInfo[];
	loadOffices: (organizationId: string, baseOffices: OfficeInfo[]) => Promise<OfficeInfo[]>;
}

export class ShellData {
	readonly #options: ShellDataOptions;

	snapshot = $state.raw<OrgSnapshot | undefined>(undefined);
	workItems = $state.raw<WorkItemSummary[]>([]);
	workError = $state.raw<unknown>(undefined);
	workErrorSeq = $state(0);
	inboxItems = $state.raw<InboxItem[]>([]);
	inboxLoadedOrganizationId = $state<string | undefined>(undefined);
	#layeredOffices = $state.raw<OfficeInfo[]>([]);
	loaded = $state(false);
	loadedOrganizationId = $state<string | undefined>(undefined);

	#loadSeq = 0;
	#leaving = false;
	#latestLoad: { seq: number; orgId: string; promise: Promise<ShellRefreshResult> } | undefined =
		undefined;

	constructor(options: ShellDataOptions) {
		this.#options = options;
	}

	get organizationId(): string {
		return this.#options.getOrganizationId();
	}

	readonly ownerId = $derived(this.snapshot ? (owner(this.snapshot)?.id ?? 'owner') : 'owner');

	readonly openInboxCount = $derived(this.inboxItems.filter((i) => i.state === 'open').length);

	readonly needsYouCount = $derived(
		this.openInboxCount > 0 ? this.openInboxCount : needsYou(this.workItems, this.ownerId)
	);

	get offices(): OfficeInfo[] {
		return this.#layeredOffices.length > 0 ? this.#layeredOffices : this.#options.getBaseOffices();
	}

	load(
		id = this.organizationId,
		reset = false
	): { promise: Promise<ShellRefreshResult>; cancel: () => void } {
		const seq = ++this.#loadSeq;
		let cancelled = false;
		if (reset) {
			this.snapshot = undefined;
			this.workItems = [];
			this.workError = undefined;
			this.inboxItems = [];
			this.inboxLoadedOrganizationId = undefined;
			this.#layeredOffices = [];
			this.loaded = false;
			this.loadedOrganizationId = undefined;
		}
		const baseOffices = untrack(() => this.#options.getBaseOffices());
		const promise: Promise<ShellRefreshResult> = Promise.allSettled([
			orgApi.view(id),
			loadShellWorkItems(id),
			inboxApi.list(id),
			this.#options.loadOffices(id, baseOffices)
		]).then(async ([orgRes, workRes, inboxRes, officesRes]): Promise<ShellRefreshResult> => {
			if (this.#leaving || cancelled) return { inboxUpdated: false, workUpdated: false };
			if (seq !== this.#loadSeq) {
				// If superseded by a newer load for the same org, await and return the winning load's publication result.
				const winning = this.#latestLoad;
				if (winning && winning.orgId === id && winning.seq > seq) {
					return await winning.promise;
				}
				return { inboxUpdated: false, workUpdated: false };
			}
			if (orgRes.status === 'fulfilled') this.snapshot = orgRes.value.snapshot;
			const workUpdated = workRes.status === 'fulfilled';
			if (workUpdated) {
				this.workItems = workRes.value;
				this.workError = undefined;
			} else {
				this.workError = workRes.reason;
				this.workErrorSeq += 1;
			}
			const inboxUpdated = inboxRes.status === 'fulfilled';
			if (inboxUpdated) {
				this.inboxItems = inboxRes.value;
				this.inboxLoadedOrganizationId = id;
			}
			this.#layeredOffices = officesRes.status === 'fulfilled' ? officesRes.value : baseOffices;
			this.loadedOrganizationId = id;
			this.loaded = true;
			return { inboxUpdated, workUpdated };
		});
		// Retain #latestLoad until overwritten by a newer load or dispose() so slower superseded loads can read its result.
		this.#latestLoad = { seq, orgId: id, promise };
		return {
			promise,
			cancel: () => {
				cancelled = true;
			}
		};
	}

	refresh(): Promise<ShellRefreshResult> {
		return this.load(this.organizationId, false).promise;
	}

	dispose(): void {
		this.#leaving = true;
		this.#latestLoad = undefined;
		this.inboxLoadedOrganizationId = undefined;
	}
}

const [getShellDataContext, setShellDataContext] = createContext<ShellData>();

export function provideShellData(options: ShellDataOptions): ShellData {
	return setShellDataContext(new ShellData(options));
}

export function useShellData(): ShellData {
	return getShellDataContext();
}
