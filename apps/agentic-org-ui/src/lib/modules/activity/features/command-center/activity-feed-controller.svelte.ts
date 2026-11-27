import { activityApi as defaultActivityApi } from '$lib/modules/activity/api';
import type { ActivityQuery, ActivityRow } from '$lib/modules/activity/types';
import { inboxApi as defaultInboxApi } from '$lib/modules/inbox/api';
import type { InboxItem } from '$lib/modules/inbox/types';
import { ACTIVITY_FEED_LIMIT, FeedChannel } from './activity-feed-channel.svelte';
import type { FeedPhase } from './assembly-model';

export { ACTIVITY_FEED_LIMIT };

export interface ActivityFeedApiLike {
	feed: (
		id: string,
		query: ActivityQuery,
		limit: number
	) => Promise<{ rows: ActivityRow[]; more: number }>;
}

export interface ActivityFeedDeps {
	activityApi?: ActivityFeedApiLike;
	inboxApi?: Pick<typeof defaultInboxApi, 'list'>;
	getOrganizationId: () => string;
	getActiveAllQuery: () => ActivityQuery;
	getActiveDecisionQuery: () => ActivityQuery;
	isDecisionTabActive: () => boolean;
	refreshShell: () => Promise<unknown>;
	onBannerMayDismiss?: (orgId: string) => void;
}

export class ActivityFeedController {
	private readonly main = new FeedChannel();
	private readonly decision = new FeedChannel();

	directInboxItems = $state.raw<InboxItem[] | undefined>(undefined);
	inboxPhase = $state<FeedPhase>('loading');
	inboxSubRetrySeq = $state(0);
	activitySubRetrySeq = $state(0);

	inboxSubFailedOrg = $state<string | undefined>(undefined);
	activitySubFailedOrg = $state<string | undefined>(undefined);
	inboxCatchupFailedOrg = $state<string | undefined>(undefined);
	reconnectingOrg = $state<string | undefined>(undefined);

	headerRetryActive = $state(false);
	shellRefreshing = $state(false);
	nowMs = $state(Date.now());

	private readonly activityApi: ActivityFeedApiLike;
	private readonly inboxApi: Pick<typeof defaultInboxApi, 'list'>;
	private readonly deps: ActivityFeedDeps;

	private inboxInFlight = false;
	private pendingInboxReconcile = false;
	private reconnectPendingInboxSub = false;
	private reconnectPendingActivitySub = false;
	private reconnectPendingInboxFetch = false;
	private inboxLoadSeq = 0;
	private shellRefreshSeq = 0;
	private destroyed = false;

	constructor(deps: ActivityFeedDeps) {
		this.deps = deps;
		this.activityApi = deps.activityApi ?? defaultActivityApi;
		this.inboxApi = deps.inboxApi ?? defaultInboxApi;
	}

	get activityRows() {
		return this.main.rows;
	}
	get feedPhase() {
		return this.main.phase;
	}
	get feedError() {
		return this.main.error;
	}
	get feedMore() {
		return this.main.more;
	}
	get feedRetrySeq() {
		return this.main.retrySeq;
	}
	get activityCatchupFailedOrg() {
		return this.main.catchupFailedOrg;
	}
	get decisionRows() {
		return this.decision.rows;
	}
	get decisionPhase() {
		return this.decision.phase;
	}
	get decisionError() {
		return this.decision.error;
	}
	get decisionMore() {
		return this.decision.more;
	}
	get decisionRetrySeq() {
		return this.decision.retrySeq;
	}
	get decisionCatchupFailedOrg() {
		return this.decision.catchupFailedOrg;
	}

	dispose(): void {
		this.destroyed = true;
		this.inboxLoadSeq += 1;
		this.main.seq += 1;
		this.decision.seq += 1;
		this.shellRefreshSeq += 1;
	}

	isBannerStillVisible(id: string): boolean {
		return (
			this.reconnectingOrg === id ||
			this.inboxSubFailedOrg === id ||
			this.activitySubFailedOrg === id ||
			this.inboxCatchupFailedOrg === id ||
			this.main.catchupFailedOrg === id ||
			(this.deps.isDecisionTabActive() && this.decision.catchupFailedOrg === id)
		);
	}

	private isStaleOrg(id: string): boolean {
		return this.destroyed || id !== this.deps.getOrganizationId();
	}

	private maybeFinishReconnect(id: string): void {
		if (this.destroyed || this.reconnectingOrg !== id) return;
		if (
			this.reconnectPendingInboxSub ||
			this.reconnectPendingActivitySub ||
			this.reconnectPendingInboxFetch ||
			this.main.reconnectPendingFetch ||
			this.decision.reconnectPendingFetch
		) {
			return;
		}
		this.reconnectingOrg = undefined;
		this.deps.onBannerMayDismiss?.(id);
	}

	fetchInbox(id: string, showLoading: boolean): void {
		const seq = ++this.inboxLoadSeq;
		this.inboxInFlight = true;
		if (showLoading) {
			this.pendingInboxReconcile = false;
			this.inboxPhase = 'loading';
			this.directInboxItems = undefined;
		}
		void this.inboxApi
			.list(id)
			.then((items) => {
				if (seq !== this.inboxLoadSeq || this.isStaleOrg(id)) return;
				this.directInboxItems = items;
				this.inboxPhase = 'ready';
				if (this.inboxCatchupFailedOrg === id) {
					this.inboxCatchupFailedOrg = undefined;
					this.deps.onBannerMayDismiss?.(id);
				}
			})
			.catch(() => {
				if (seq !== this.inboxLoadSeq || this.isStaleOrg(id)) return;
				if (!showLoading && this.pendingInboxReconcile) return;
				this.directInboxItems = undefined;
				this.inboxPhase = 'error';
				if (!showLoading) this.inboxCatchupFailedOrg = id;
			})
			.finally(() => {
				if (seq !== this.inboxLoadSeq || this.isStaleOrg(id)) return;
				this.inboxInFlight = false;
				if (this.pendingInboxReconcile) {
					this.pendingInboxReconcile = false;
					this.fetchInbox(id, false);
				} else {
					this.reconnectPendingInboxFetch = false;
					this.maybeFinishReconnect(id);
					if (!this.shellRefreshing) this.headerRetryActive = false;
				}
			});
	}

	scheduleInboxReconcile(id: string): void {
		if (this.inboxInFlight) this.pendingInboxReconcile = true;
		else this.fetchInbox(id, false);
	}

	resetInboxScope(): void {
		this.inboxLoadSeq += 1;
		this.shellRefreshSeq += 1;
		this.inboxInFlight = false;
		this.pendingInboxReconcile = false;
		this.reconnectPendingInboxSub = false;
		this.reconnectPendingActivitySub = false;
		this.reconnectPendingInboxFetch = false;
		this.main.reconnectPendingFetch = false;
		this.decision.reconnectPendingFetch = false;
		this.headerRetryActive = false;
		this.shellRefreshing = false;
	}

	onInboxSubscribed(id: string): void {
		if (this.inboxSubFailedOrg === id) {
			this.inboxSubFailedOrg = undefined;
			this.deps.onBannerMayDismiss?.(id);
		}
		if (this.reconnectingOrg === id) this.reconnectPendingInboxFetch = true;
		this.reconnectPendingInboxSub = false;
		this.scheduleInboxReconcile(id);
		this.maybeFinishReconnect(id);
	}

	onInboxSubscribeFailed(id: string): void {
		if (this.destroyed) return;
		this.inboxSubFailedOrg = id;
		this.reconnectPendingInboxSub = false;
		this.maybeFinishReconnect(id);
	}

	private handleChannelFailure(
		channel: FeedChannel,
		id: string,
		query: ActivityQuery,
		showLoading: boolean,
		err: unknown,
		isActive: () => boolean,
		getNextQuery: () => ActivityQuery,
		fallbackMessage: string
	): void {
		if (!showLoading && channel.phase === 'ready') {
			if (channel.applySoftCatchupFailure(query)) this.nowMs = Date.now();
			if (channel.pendingReconcile && isActive()) {
				channel.pendingReconcile = false;
				this.fetchChannel(
					channel,
					id,
					getNextQuery(),
					false,
					false,
					isActive,
					getNextQuery,
					fallbackMessage
				);
			} else {
				channel.pendingReconcile = false;
				channel.catchupFailedOrg = id;
				channel.reconnectPendingFetch = false;
				this.maybeFinishReconnect(id);
			}
			return;
		}
		if (channel.pendingReconcile && isActive()) {
			channel.pendingReconcile = false;
			this.fetchChannel(
				channel,
				id,
				getNextQuery(),
				false,
				true,
				isActive,
				getNextQuery,
				fallbackMessage
			);
			return;
		}
		channel.applyHardFailure(id, showLoading, err, fallbackMessage);
		this.nowMs = Date.now();
		channel.reconnectPendingFetch = false;
		this.maybeFinishReconnect(id);
	}

	private fetchChannel(
		channel: FeedChannel,
		id: string,
		query: ActivityQuery,
		showLoading: boolean,
		preserveBuffer: boolean,
		isActive: () => boolean,
		getNextQuery: () => ActivityQuery,
		fallbackMessage: string
	): void {
		const seq = channel.beginFetch(showLoading, preserveBuffer);
		void this.activityApi
			.feed(id, query, ACTIVITY_FEED_LIMIT)
			.then((res) => {
				if (seq !== channel.seq || this.isStaleOrg(id)) return;
				if (channel.applySuccess(id, query, res)) this.deps.onBannerMayDismiss?.(id);
				this.nowMs = Date.now();
				if (channel.pendingReconcile && isActive()) {
					channel.pendingReconcile = false;
					this.fetchChannel(
						channel,
						id,
						getNextQuery(),
						false,
						false,
						isActive,
						getNextQuery,
						fallbackMessage
					);
				} else {
					channel.pendingReconcile = false;
					channel.reconnectPendingFetch = false;
					this.maybeFinishReconnect(id);
				}
			})
			.catch((err: unknown) => {
				if (seq !== channel.seq || this.isStaleOrg(id)) return;
				this.handleChannelFailure(
					channel,
					id,
					query,
					showLoading,
					err,
					isActive,
					getNextQuery,
					fallbackMessage
				);
			});
	}

	fetchMainFeed(
		id: string,
		query: ActivityQuery,
		showLoading: boolean,
		preserveBuffer = false
	): void {
		this.fetchChannel(
			this.main,
			id,
			query,
			showLoading,
			preserveBuffer,
			() => true,
			() => this.deps.getActiveAllQuery(),
			'Could not load the activity feed.'
		);
	}

	resetMainScope(): void {
		this.main.reset();
	}

	fetchDecisionFeed(
		id: string,
		query: ActivityQuery,
		showLoading: boolean,
		preserveBuffer = false
	): void {
		this.fetchChannel(
			this.decision,
			id,
			query,
			showLoading,
			preserveBuffer,
			() => this.deps.isDecisionTabActive(),
			() => this.deps.getActiveDecisionQuery(),
			'Could not load decisions.'
		);
	}

	resetDecisionScope(id: string): void {
		this.decision.reset();
		if (this.decision.reconnectPendingFetch) {
			this.decision.reconnectPendingFetch = false;
			this.maybeFinishReconnect(id);
		}
	}

	handleLiveRow(row: ActivityRow): void {
		if (this.main.applyLiveRow(row, this.deps.getActiveAllQuery())) this.nowMs = Date.now();
		if (
			this.deps.isDecisionTabActive() &&
			this.decision.applyLiveRow(row, this.deps.getActiveDecisionQuery())
		) {
			this.nowMs = Date.now();
		}
	}

	private reconcileOnSubscribe(
		channel: FeedChannel,
		trackingReconnect: boolean,
		trigger: () => void
	): void {
		if (channel.inFlight) {
			if (trackingReconnect) channel.reconnectPendingFetch = true;
			channel.pendingReconcile = true;
		} else if (channel.phase === 'ready' || channel.phase === 'error') {
			if (trackingReconnect) channel.reconnectPendingFetch = true;
			trigger();
		}
	}

	onActivitySubscribed(id: string): void {
		if (this.activitySubFailedOrg === id) {
			this.activitySubFailedOrg = undefined;
			this.deps.onBannerMayDismiss?.(id);
		}
		const tracking = this.reconnectingOrg === id;
		this.reconcileOnSubscribe(this.main, tracking, () =>
			this.fetchMainFeed(id, this.deps.getActiveAllQuery(), false)
		);
		if (this.deps.isDecisionTabActive()) {
			this.reconcileOnSubscribe(this.decision, tracking, () =>
				this.fetchDecisionFeed(id, this.deps.getActiveDecisionQuery(), false)
			);
		}
		this.reconnectPendingActivitySub = false;
		this.maybeFinishReconnect(id);
	}

	onActivitySubscribeFailed(id: string): void {
		if (this.destroyed) return;
		this.activitySubFailedOrg = id;
		this.reconnectPendingActivitySub = false;
		this.maybeFinishReconnect(id);
	}

	retryFeed(): void {
		if (this.activitySubFailedOrg === this.deps.getOrganizationId()) this.activitySubRetrySeq += 1;
		this.main.retrySeq += 1;
	}

	retryDecisions(): void {
		if (this.activitySubFailedOrg === this.deps.getOrganizationId()) this.activitySubRetrySeq += 1;
		this.decision.retrySeq += 1;
	}

	reconnectLiveUpdates(): void {
		const id = this.deps.getOrganizationId();
		if (this.reconnectingOrg === id) return;
		this.reconnectingOrg = id;

		if (this.inboxSubFailedOrg === id) {
			this.reconnectPendingInboxSub = true;
			this.inboxSubRetrySeq += 1;
		} else if (this.inboxCatchupFailedOrg === id) {
			this.reconnectPendingInboxFetch = true;
			this.scheduleInboxReconcile(id);
		}

		if (this.activitySubFailedOrg === id) {
			this.reconnectPendingActivitySub = true;
			this.activitySubRetrySeq += 1;
		} else {
			if (this.main.catchupFailedOrg === id) {
				this.main.reconnectPendingFetch = true;
				if (this.main.inFlight) this.main.pendingReconcile = true;
				else this.fetchMainFeed(id, this.deps.getActiveAllQuery(), false);
			}
			if (this.decision.catchupFailedOrg === id && this.deps.isDecisionTabActive()) {
				this.decision.reconnectPendingFetch = true;
				if (this.decision.inFlight) this.decision.pendingReconcile = true;
				else this.fetchDecisionFeed(id, this.deps.getActiveDecisionQuery(), false);
			}
		}

		this.maybeFinishReconnect(id);
	}

	retryShellAndInbox(): void {
		const id = this.deps.getOrganizationId();
		if (this.inboxSubFailedOrg === id) this.inboxSubRetrySeq += 1;
		if (this.activitySubFailedOrg === id) this.activitySubRetrySeq += 1;
		this.fetchInbox(id, true);
		const refreshSeq = ++this.shellRefreshSeq;
		this.shellRefreshing = true;
		void this.deps.refreshShell().finally(() => {
			if (this.isStaleOrg(id) || refreshSeq !== this.shellRefreshSeq) return;
			this.shellRefreshing = false;
			if (!this.inboxInFlight) this.headerRetryActive = false;
		});
	}
}
