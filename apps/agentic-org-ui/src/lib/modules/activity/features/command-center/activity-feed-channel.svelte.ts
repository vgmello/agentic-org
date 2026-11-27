import type { ActivityQuery, ActivityRow } from '$lib/modules/activity/types';
import { mergeLiveActivityRows, type FeedPhase } from './assembly-model';

export const ACTIVITY_FEED_LIMIT = 200;

export class FeedChannel {
	rows = $state.raw<ActivityRow[] | undefined>(undefined);
	phase = $state<FeedPhase>('loading');
	error = $state<string | undefined>(undefined);
	more = $state(0);
	retrySeq = $state(0);
	catchupFailedOrg = $state<string | undefined>(undefined);

	buffered: ActivityRow[] = [];
	inFlight = false;
	pendingReconcile = false;
	reconnectPendingFetch = false;
	seq = 0;

	reset(): void {
		this.seq += 1;
		this.inFlight = false;
		this.pendingReconcile = false;
		this.buffered = [];
	}

	beginFetch(showLoading: boolean, preserveBuffer: boolean): number {
		const nextSeq = ++this.seq;
		this.inFlight = true;
		if (!preserveBuffer) this.buffered = [];
		if (showLoading) {
			this.pendingReconcile = false;
			this.phase = 'loading';
			this.error = undefined;
			this.more = 0;
			this.rows = undefined;
		}
		return nextSeq;
	}

	applySuccess(
		id: string,
		query: ActivityQuery,
		res: { rows: ActivityRow[]; more: number }
	): boolean {
		const merged = mergeLiveActivityRows(
			res.rows,
			this.buffered,
			query,
			ACTIVITY_FEED_LIMIT,
			res.more
		);
		this.buffered = [];
		this.rows = merged.rows;
		this.more = merged.more;
		this.phase = 'ready';
		this.error = undefined;
		const clearedCatchup = this.catchupFailedOrg === id;
		if (clearedCatchup) this.catchupFailedOrg = undefined;
		this.inFlight = false;
		return clearedCatchup;
	}

	applySoftCatchupFailure(query: ActivityQuery): boolean {
		const merged = mergeLiveActivityRows(
			this.rows ?? [],
			this.buffered,
			query,
			ACTIVITY_FEED_LIMIT,
			this.more
		);
		this.buffered = [];
		if (merged.changed) {
			this.rows = merged.rows;
			this.more = merged.more;
		}
		this.inFlight = false;
		return merged.changed;
	}

	applyHardFailure(id: string, showLoading: boolean, err: unknown, fallbackMessage: string): void {
		this.pendingReconcile = false;
		this.buffered = [];
		this.inFlight = false;
		this.rows = undefined;
		this.more = 0;
		this.phase = 'error';
		this.error = err instanceof Error && err.message ? err.message : fallbackMessage;
		if (!showLoading) this.catchupFailedOrg = id;
	}

	applyLiveRow(row: ActivityRow, query: ActivityQuery): boolean {
		if (this.inFlight) {
			this.buffered.push(row);
			return false;
		}
		if (this.phase !== 'ready') return false;
		const merged = mergeLiveActivityRows(
			this.rows ?? [],
			[row],
			query,
			ACTIVITY_FEED_LIMIT,
			this.more
		);
		if (!merged.changed) return false;
		this.rows = merged.rows;
		this.more = merged.more;
		return true;
	}
}
