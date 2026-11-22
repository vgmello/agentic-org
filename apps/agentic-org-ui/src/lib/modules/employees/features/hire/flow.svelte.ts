// Where a hire stands in the flow (our-org-in-the-app §7), and the rules for moving between its steps: what the hirer
// typed is kept across Back (app-shell-ux §6a), and the HR lead's answer is applied only while the flow waits for it,
// never over a form the hirer chose to fill in themselves (STAFF-5).
import type { HireProposal, HireRequest } from '../../types';

export type Step = 'describe' | 'drafting' | 'proposal' | 'review' | 'hiring' | 'closed';

// Empty strings, not undefined: a select bound to undefined picks its first option, a made-up default.
const EMPTY: Partial<HireProposal> = { executionProfileId: '', model: '', officeId: '' };

export class HireFlowState {
	step = $state<Step>('describe');
	/** The hirer fills in the details: no HR proposal, reason or suggestion. */
	manual = $state(false);
	/** What was typed in Describe. */
	reason = $state('');
	fields = $state<Partial<HireProposal>>({});
	request = $state.raw<HireRequest | undefined>();
	/** The form the hirer filled in by hand, kept when they go back to Describe. */
	#typed: Partial<HireProposal> | undefined;

	/** A request picked up from the address or the requests list. */
	open(r: HireRequest): void {
		this.request = r;
		this.reason = r.reason;
		// Only its hirer confirms a draft (§7.1): someone else's opens read-only.
		if (r.state === 'drafting' && !r.byViewer) this.step = 'closed';
		else if (r.state === 'drafting' && r.drafting?.state === 'ready') this.#fromProposal(r);
		else if (r.state === 'drafting') this.step = 'drafting';
		else if (
			r.state === 'hiring' ||
			r.state === 'hired' ||
			(r.state === 'failed' && r.doneSteps.length)
		)
			this.step = 'hiring';
		else this.step = 'closed';
	}

	/** The HR lead was asked (again): the flow waits for its answer. */
	asked(r: HireRequest): void {
		this.request = r;
		this.manual = false;
		this.step = 'drafting';
	}

	/** A newer reading of the drafting request (reading, still starting, stopped), while the flow waits on it. */
	updated(r: HireRequest): void {
		if (this.step === 'drafting' && r.id === this.request?.id) this.request = r;
	}

	/**
	 * The HR lead's proposal arrived. It is applied only while the flow still waits for it: an answer that comes after
	 * the hirer chose Fill in the details yourself, or went back, changes nothing. Says whether it was applied.
	 */
	hrAnswered(r: HireRequest): boolean {
		if (this.step !== 'drafting' || this.manual || r.id !== this.request?.id) return false;
		this.request = r;
		this.#fromProposal(r);
		return true;
	}

	/** Fill in the details yourself: name and handle empty, or what the hirer already typed here. */
	fillIn(homeTeamId: string): void {
		this.manual = true;
		this.fields = { name: '', handle: '', ...EMPTY, ...this.#typed, homeTeamId };
		this.step = 'proposal';
	}

	/** One step back, keeping what was typed. */
	back(): void {
		if (this.step === 'review') this.step = 'proposal';
		else if (this.step === 'proposal' || this.step === 'drafting') {
			if (this.step === 'proposal' && this.manual) this.#typed = $state.snapshot(this.fields);
			this.step = 'describe';
		}
	}

	/**
	 * Next on Describe when nothing changed since the HR lead was asked: the step to return to, without asking again.
	 */
	unchanged(homeTeamId: string): 'proposal' | 'drafting' | undefined {
		const r = this.request;
		if (r?.state !== 'drafting' || r.reason !== this.reason.trim() || r.homeTeam.id !== homeTeamId)
			return undefined;
		// The hirer was filling in the details: back to that form, never to a wait the HR lead's answer can't end.
		if (this.manual) return 'proposal';
		if (r.drafting?.state === 'ready') return 'proposal';
		return r.drafting?.state === 'reading' ? 'drafting' : undefined;
	}

	confirmed(r: HireRequest): void {
		this.request = r;
		this.step = 'hiring';
	}

	reset(): void {
		this.request = undefined;
		this.manual = false;
		this.fields = {};
		this.reason = '';
		this.#typed = undefined;
		this.step = 'describe';
	}

	#fromProposal(r: HireRequest): void {
		this.manual = false;
		this.fields = { ...EMPTY, ...r.proposal, homeTeamId: r.proposal?.homeTeamId ?? r.homeTeam.id };
		this.step = 'proposal';
	}
}
