import { unavailableCall } from '$lib/platform/unavailable-api';
import type {
	AssignmentChange,
	BudgetAction,
	BudgetOperation,
	BudgetUnit,
	Choices,
	CloseAs,
	CreateOperation,
	DraftTask,
	FixHow,
	NewTask,
	Priority,
	PrivateNote,
	Severity,
	TaskCost,
	TaskDetail,
	TaskLink,
	TaskRow,
	TaskStatus,
	WhyLink,
	WorkQueue,
	WorkScope
} from './types';

/**
 * What the Work screens read and change (work-items-ux, feature-requests-ux, costs-and-insights-ux §2 and §4). Every
 * change is one accepted operation at the task service, made against the task's `expectedRevision`, recorded as an
 * immutable event with the admitted actor (ADR-0031), and authorized there whatever the screen shows.
 *
 * Refusals reject with an AppError: `not_found` for a reference that finds no task, `not_authorized`, `conflict` when
 * the task moved on (an assignment conflict also carries `current`, an `AssignmentConflict`), and a refusal about one
 * form field carries `field` (a `TaskField`). A write whose outcome is unknown rejects with `outcome_unknown` and is
 * resolved through the same operation ID, never by sending a second create.
 */
export interface WorkItemsApi {
	/** The task projection for a scope: board, list and Needs me read the same rows. */
	queue(organizationId: string, scope: WorkScope): Promise<WorkQueue>;
	/** Looks a task up by its reference (`T-A42`); one task or `not_found`. */
	task(organizationId: string, ref: string): Promise<TaskDetail>;
	/** Teams, employees and projects a capture or assignment may name. */
	choices(organizationId: string): Promise<Choices>;
	/** Up to three similar open tasks of `type` in the scope, by title (BUG-3, RQS-13). */
	similar(
		organizationId: string,
		query: {
			type: 'bug' | 'request';
			text: string;
			projectId: string | null;
			teamId: string | null;
		}
	): Promise<TaskRow[]>;
	/** Finds a task to name as a duplicate original, merge target or fix task; `not_found` when none. */
	lookup(organizationId: string, ref: string): Promise<TaskLink>;

	create(organizationId: string, draft: NewTask): Promise<{ id: string; ref: string }>;
	/** Where a create whose outcome was unknown stands, by its operation ID (RQS-17). */
	createOperation(organizationId: string, operationId: string): Promise<CreateOperation>;
	/** "This is the same bug" / "Add to…": an explicitly shared comment on the existing task, its reporter kept. */
	addToExisting(
		organizationId: string,
		taskId: string,
		change: { text: string; operationId: string }
	): Promise<{ ref: string }>;

	setStatus(
		organizationId: string,
		taskId: string,
		change: { status: TaskStatus; blockedReason?: string; expectedRevision: number }
	): Promise<void>;
	setPriority(
		organizationId: string,
		taskId: string,
		change: { priority: Priority; expectedRevision: number }
	): Promise<void>;
	edit(
		organizationId: string,
		taskId: string,
		change: { title: string; description: string; expectedRevision: number }
	): Promise<void>;
	/** Both fields together, explicit nulls, at the expected revision; a stale write changes neither (TSK-13). */
	assign(organizationId: string, taskId: string, change: AssignmentChange): Promise<void>;
	/** Confirms severity and priority and saves the assignment pair in one change (TRI-2). */
	triage(
		organizationId: string,
		taskId: string,
		change: AssignmentChange & { severity: Severity | null; priority: Priority }
	): Promise<void>;

	addSteps(organizationId: string, taskId: string, texts: string[]): Promise<void>;
	setStep(
		organizationId: string,
		taskId: string,
		stepId: string,
		change: { done?: boolean; required?: boolean; expectedRevision: number }
	): Promise<void>;
	moveStep(organizationId: string, taskId: string, stepId: string, toIndex: number): Promise<void>;
	removeStep(organizationId: string, taskId: string, stepId: string): Promise<void>;

	comment(organizationId: string, taskId: string, body: string): Promise<void>;

	/** The private store, separate from the task (PN-01..06). Agents never read another author's notes. */
	notes(organizationId: string, taskId: string): Promise<PrivateNote[]>;
	addNote(organizationId: string, taskId: string, body: string): Promise<void>;
	editNote(
		organizationId: string,
		taskId: string,
		noteId: string,
		change: { body: string; expectedRevision: number }
	): Promise<void>;
	deleteNote(organizationId: string, taskId: string, noteId: string): Promise<void>;
	/** Publishes a copy as a new shared comment after the author confirmed it; the private original stays. */
	publishNote(organizationId: string, taskId: string, noteId: string): Promise<void>;

	markFixed(
		organizationId: string,
		taskId: string,
		change: { how: FixHow; note: string; expectedRevision: number }
	): Promise<void>;
	/** Fixed records a verification; Still happens needs a note, reopens and counts it once (FIX-5). */
	verify(
		organizationId: string,
		taskId: string,
		change: { result: 'fixed' | 'still_happens'; note: string; expectedRevision: number }
	): Promise<void>;
	close(
		organizationId: string,
		taskId: string,
		change: CloseAs & { expectedRevision: number }
	): Promise<void>;
	reopen(
		organizationId: string,
		taskId: string,
		change: { note: string; expectedRevision: number }
	): Promise<void>;

	/** Creates exactly the previewed drafts, linked to the request, in one recoverable operation (RQS-9). */
	approve(
		organizationId: string,
		taskId: string,
		change: { drafts: DraftTask[]; operationId: string; expectedRevision: number }
	): Promise<{ created: TaskLink[] }>;
	askForChanges(
		organizationId: string,
		taskId: string,
		change: { comment: string; expectedRevision: number }
	): Promise<void>;
	decline(
		organizationId: string,
		taskId: string,
		change: { reason: string; expectedRevision: number }
	): Promise<void>;
	merge(
		organizationId: string,
		taskId: string,
		change: { intoRef: string; expectedRevision: number }
	): Promise<void>;

	/** Documents the Why block may link: decisions, designs, specs and evaluations the viewer can read. */
	whyChoices(organizationId: string, taskId: string): Promise<WhyLink[]>;
	linkWhy(organizationId: string, taskId: string, documentId: string): Promise<void>;

	/** The Cost and time block: amounts for the owner or project manager, the budget state for everyone else. */
	cost(organizationId: string, taskId: string): Promise<TaskCost>;
	/** Sets or changes the task's lifetime limit: a positive integer in `unit`. Idempotent on `operationId`. */
	setBudget(
		organizationId: string,
		taskId: string,
		change: { unit: BudgetUnit; limit: number; expectedPolicyRevision: number; operationId: string }
	): Promise<void>;
	/** A Budget reached action. Stop requests Cancel through the task transition service. */
	budgetAction(
		organizationId: string,
		taskId: string,
		change: { action: BudgetAction; reason?: string; operationId: string }
	): Promise<void>;
	/** Where a budget write whose outcome was unknown stands, by its operation ID. */
	budgetOperation(organizationId: string, operationId: string): Promise<BudgetOperation>;
}

// The web API does not serve work items yet: every call refuses, and each place says so.
// Under `bun run dev:mock`, e2e/fakes/modules/workitems.ts stands in for this file.
export const workItemsApi: WorkItemsApi = {
	queue: unavailableCall,
	task: unavailableCall,
	choices: unavailableCall,
	similar: unavailableCall,
	lookup: unavailableCall,
	create: unavailableCall,
	createOperation: unavailableCall,
	addToExisting: unavailableCall,
	setStatus: unavailableCall,
	setPriority: unavailableCall,
	edit: unavailableCall,
	assign: unavailableCall,
	triage: unavailableCall,
	addSteps: unavailableCall,
	setStep: unavailableCall,
	moveStep: unavailableCall,
	removeStep: unavailableCall,
	comment: unavailableCall,
	notes: unavailableCall,
	addNote: unavailableCall,
	editNote: unavailableCall,
	deleteNote: unavailableCall,
	publishNote: unavailableCall,
	markFixed: unavailableCall,
	verify: unavailableCall,
	close: unavailableCall,
	reopen: unavailableCall,
	approve: unavailableCall,
	askForChanges: unavailableCall,
	decline: unavailableCall,
	merge: unavailableCall,
	whyChoices: unavailableCall,
	linkWhy: unavailableCall,
	cost: unavailableCall,
	setBudget: unavailableCall,
	budgetAction: unavailableCall,
	budgetOperation: unavailableCall
};
