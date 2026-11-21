import type {
	RequestSimilarQuery,
	WorkAccepted,
	WorkEvent,
	WorkItem,
	WorkItemSummary,
	WorkPosition
} from '$lib/proto';
import type { WorkCall } from './engine';

/**
 * What the work screens need from an office: `work.snapshot`, `work.get` and the item calls.
 * The calls are `org_proto::work`. Production defaults live in `src/lib/work/index.ts`;
 * dev/test mocks live in `src/lib/work/mock.ts` and `e2e/fakes/work-api.ts`.
 */
export interface WorkApi {
	/** `closedSince`: Done tasks that went Done at or after it (`work.snapshot { closed_since }`); all of them when absent. */
	snapshot(
		organizationId: string,
		projectId?: string,
		closedSince?: string
	): Promise<{ position: WorkPosition; items: WorkItemSummary[] }>;
	get(organizationId: string, itemId: string): Promise<WorkItem>;
	/** Rejects with the error the office gave (an `AppError` with source `api`). */
	command(organizationId: string, call: WorkCall): Promise<WorkAccepted>;
	/** `request.similar`: open requests that read like some words (read-only). */
	similar(
		organizationId: string,
		query: RequestSimilarQuery
	): Promise<{ items: WorkItemSummary[] }>;
	/** `work.activity`: what happened to one item, oldest first. */
	activity(organizationId: string, itemId: string): Promise<WorkEvent[]>;
	/** Whether this office keeps feature requests yet (the Rust shell's mock office does not). */
	readonly requests: boolean;
}
