import { untrack } from 'svelte';
import type { OfficeInfo, OrgSnapshot, ProjectTeam, WorkItemSummary } from '$lib/proto';
import { inboxApi } from '$lib/modules/inbox/api';
import type { InboxItem } from '$lib/platform/workspace/inbox-types';
import {
	mergeProjectHealthOverlayMap,
	stripVolatileOverlayMap,
	type ProjectHealthOverlay,
	type ProjectHealthOverlayUpdateOptions
} from '$lib/platform/workspace/project-health';
import {
	setShellDataContext,
	useShellData,
	type ShellDataContext,
	type ShellRefreshResult
} from '$lib/platform/workspace/shell-data-context';
import { orgApi } from '$lib/modules/organization/api';
import { owner } from '$lib/modules/organization/model';
import { needsYou } from '$lib/modules/work/saved';
import { loadShellWorkItems } from '$lib/modules/workitems/shell-summary';

export { loadShellWorkItems, useShellData, type ShellRefreshResult };

export interface ShellDataOptions {
	getOrganizationId: () => string;
	getBaseOffices: () => OfficeInfo[];
	loadOffices: (
		organizationId: string,
		baseOffices: OfficeInfo[],
		signal?: AbortSignal
	) => Promise<OfficeInfo[]>;
}

export class ShellData implements ShellDataContext {
	readonly #options: ShellDataOptions;

	snapshot = $state.raw<OrgSnapshot | undefined>(undefined);
	workItems = $state.raw<WorkItemSummary[]>([]);
	workError = $state.raw<unknown>(undefined);
	workErrorSeq = $state(0);
	inboxItems = $state.raw<InboxItem[]>([]);
	inboxLoadedOrganizationId = $state<string | undefined>(undefined);
	#layeredOffices = $state.raw<OfficeInfo[]>([]);
	#rawProjectHealthById = $state.raw<Readonly<Record<string, ProjectHealthOverlay>>>({});
	loaded = $state(false);
	loadedOrganizationId = $state<string | undefined>(undefined);

	#loadSeq = 0;
	#leaving = false;
	#activeAbort: AbortController | undefined;
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

	readonly #knownTeamLookup = $derived.by((): Readonly<Record<string, true>> | undefined => {
		const snap = this.snapshot;
		if (!snap) return undefined;
		const lookup: Record<string, true> = {};
		for (const team of snap.teams) lookup[team.id] = true;
		return lookup;
	});

	readonly validProjectTeams = $derived.by((): readonly ProjectTeam[] | undefined => {
		const snap = this.snapshot;
		const lookup = this.#knownTeamLookup;
		if (!snap || !lookup) return undefined;
		return (snap.project_teams ?? []).filter((pt) => Boolean(lookup[pt.team_id]));
	});

	readonly projectHealthById = $derived.by((): Readonly<Record<string, ProjectHealthOverlay>> => {
		const raw = this.#rawProjectHealthById;
		const lookup = this.#knownTeamLookup;
		if (!lookup) return raw;
		const filtered: Record<string, ProjectHealthOverlay> = {};
		for (const [id, overlay] of Object.entries(raw)) {
			if (overlay.teamIds === undefined) {
				filtered[id] = overlay;
			} else {
				filtered[id] = Object.freeze({
					...overlay,
					teamIds: Object.freeze(overlay.teamIds.filter((teamId) => Boolean(lookup[teamId])))
				});
			}
		}
		return Object.freeze(filtered);
	});

	personPanelContext() {
		const id = this.organizationId;
		return {
			organizationId: id,
			snapshot: this.snapshot,
			workItems: this.workItems,
			workLoaded: this.loaded && this.loadedOrganizationId === id && this.workError === undefined
		};
	}

	setProjectHealthOverlays(
		organizationId: string,
		overlays: Readonly<Record<string, ProjectHealthOverlay>>,
		options?: ProjectHealthOverlayUpdateOptions
	): void {
		if (this.#leaving || organizationId !== this.organizationId) return;
		this.#rawProjectHealthById = mergeProjectHealthOverlayMap(
			this.#rawProjectHealthById,
			overlays,
			options
		);
	}

	clearVolatileProjectHealthCounts(organizationId = this.organizationId): void {
		if (this.#leaving || organizationId !== this.organizationId) return;
		this.#rawProjectHealthById = stripVolatileOverlayMap(this.#rawProjectHealthById);
	}

	#resetFields(): void {
		this.snapshot = undefined;
		this.workItems = [];
		this.workError = undefined;
		this.inboxItems = [];
		this.inboxLoadedOrganizationId = undefined;
		this.#layeredOffices = [];
		this.#rawProjectHealthById = {};
		this.loaded = false;
		this.loadedOrganizationId = undefined;
	}

	#applyLoadResults(
		id: string,
		baseOffices: OfficeInfo[],
		orgRes: PromiseSettledResult<{ snapshot: OrgSnapshot }>,
		workRes: PromiseSettledResult<WorkItemSummary[]>,
		inboxRes: PromiseSettledResult<InboxItem[]>,
		officesRes: PromiseSettledResult<OfficeInfo[]>
	): ShellRefreshResult {
		if (orgRes.status === 'fulfilled') this.snapshot = orgRes.value.snapshot;
		const workUpdated = workRes.status === 'fulfilled';
		if (workUpdated) {
			this.workItems = workRes.value;
			this.workError = undefined;
			this.#rawProjectHealthById = stripVolatileOverlayMap(this.#rawProjectHealthById);
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
	}

	#awaitSuperseded(id: string, seq: number): Promise<ShellRefreshResult> {
		const winning = this.#latestLoad;
		if (winning && winning.orgId === id && winning.seq > seq) {
			return winning.promise;
		}
		return Promise.resolve({ inboxUpdated: false, workUpdated: false });
	}

	load(
		id = this.organizationId,
		reset = false
	): { promise: Promise<ShellRefreshResult>; cancel: () => void } {
		this.#activeAbort?.abort();
		const controller = new AbortController();
		this.#activeAbort = controller;
		const seq = ++this.#loadSeq;
		let cancelled = false;
		if (reset) this.#resetFields();
		const baseOffices = untrack(() => this.#options.getBaseOffices());
		const promise: Promise<ShellRefreshResult> = Promise.allSettled([
			orgApi.view(id, controller.signal),
			loadShellWorkItems(id, controller.signal),
			inboxApi.list(id, controller.signal),
			this.#options.loadOffices(id, baseOffices, controller.signal)
		]).then(async ([orgRes, workRes, inboxRes, officesRes]): Promise<ShellRefreshResult> => {
			if (this.#activeAbort === controller) this.#activeAbort = undefined;
			if (this.#leaving || cancelled) return { inboxUpdated: false, workUpdated: false };
			if (controller.signal.aborted || seq !== this.#loadSeq) {
				return await this.#awaitSuperseded(id, seq);
			}
			return this.#applyLoadResults(id, baseOffices, orgRes, workRes, inboxRes, officesRes);
		});
		// Retain #latestLoad until overwritten by a newer load or dispose() so slower superseded loads can read its result.
		this.#latestLoad = { seq, orgId: id, promise };
		return {
			promise,
			cancel: () => {
				cancelled = true;
				controller.abort();
			}
		};
	}

	refresh(): Promise<ShellRefreshResult> {
		return this.load(this.organizationId, false).promise;
	}

	dispose(): void {
		this.#leaving = true;
		this.#activeAbort?.abort();
		this.#activeAbort = undefined;
		this.#latestLoad = undefined;
		this.inboxLoadedOrganizationId = undefined;
	}
}

export function provideShellData(options: ShellDataOptions): ShellData {
	const data = new ShellData(options);
	setShellDataContext(data);
	return data;
}
