/** A project card on Home: real counts only. Lead, health and due date appear only when the project has them. */
export interface HomeProject {
	id: string;
	name: string;
	/** Tasks done and all tasks counted by the project/work progress definition (cancelled work is not counted). */
	done: number;
	total: number;
	open: number;
	blocked: number;
	members: number;
	/** Present only when the project has a lead of its own: never inferred from contributor teams. */
	lead?: { id: string; name: string };
	/** A calendar date, `YYYY-MM-DD`, present only when the project has one. */
	due?: string;
}

/** One agent observed working now, with the task it is on. */
export interface WorkingAgent {
	member: { id: string; name: string };
	task?: { ref: string; title: string };
	/** An instant: when this work was first observed. */
	since: string;
}

export interface WorkingNow {
	/** False when live presence is unavailable: Home says so instead of an empty list. */
	live: boolean;
	agents: WorkingAgent[];
}

/** "This month: ≈ $412 · 63 tasks done · 30 h waiting on you". */
export interface MonthSummary {
	cost: number;
	tasksDone: number;
	waitingHours: number;
}
