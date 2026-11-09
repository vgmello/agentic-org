// What an import has made, kept across runs, so a later run knows its own work (the team the owner renamed, the agent
// whose lines the owner changed) and doesn't make it again or undo the owner's edits. Per organization, in this app's
// storage. Without it a run adopts whatever it finds in the organization as the owner's own and makes only what is not
// there; it never fills in or changes what it finds.

/** A team the import made or adopted. `led`: its lead has been dealt with (set, or already had one). */
export interface LedgerTeam {
	id: string;
	led: boolean;
}

/** An agent the import made or adopted. `linked`: its team and manager have been dealt with. */
export interface LedgerAgent {
	id: string;
	/** The import made it (rather than finding it already there): its team is added even if it has others. */
	created: boolean;
	linked: boolean;
}

export interface Ledger {
	/** By team key (the id of the team in the plan). */
	teams: Record<string, LedgerTeam>;
	/** By messaging name, in lower case. */
	agents: Record<string, LedgerAgent>;
	project?: { id: string };
}

export interface LedgerStore {
	load(): Ledger;
	save(ledger: Ledger): void;
}

export const emptyLedger = (): Ledger => ({ teams: {}, agents: {} });

export function memoryLedger(): LedgerStore {
	let kept = emptyLedger();
	return { load: () => structuredClone(kept), save: (ledger) => void (kept = structuredClone(ledger)) };
}

const isLedger = (x: unknown): x is Ledger => !!x && typeof x === 'object' && typeof (x as Ledger).teams === 'object' && (x as Ledger).teams !== null && typeof (x as Ledger).agents === 'object' && (x as Ledger).agents !== null;

/** Kept in the browser's storage under the organization's id; if that isn't available the ledger lives for this run only. */
export function browserLedger(organizationId: string): LedgerStore {
	const key = `agentic-org.import.${organizationId}`;
	const memory = memoryLedger();
	return {
		load() {
			try {
				const found = JSON.parse(localStorage.getItem(key) ?? 'null');
				if (isLedger(found)) return found;
			} catch {
				// unreadable or unavailable: start empty
			}
			return memory.load();
		},
		save(ledger) {
			memory.save(ledger);
			try {
				localStorage.setItem(key, JSON.stringify(ledger));
			} catch {
				// not kept past this run
			}
		}
	};
}
