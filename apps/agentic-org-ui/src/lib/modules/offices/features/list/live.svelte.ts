// The live side of the Offices list (offices-and-teams-ux §3.1, §3.3, OFC-11, OFC-12, OFC-46): whether the office this
// app is connected to answers, what the host says it can do, the statuses it pushes, and the restart countdowns. Created
// while the page component initializes; `watch()` ties its effects to the page and `start()` runs from the page's mount.
import { untrack } from 'svelte';
import { officeApi } from '$lib/modules/offices/office-api';
import { createProbe } from '$lib/modules/offices/probe';
import {
	applyStatusUpdate,
	displayStatusFor,
	sampleRestartDeadlines,
	whyForLabel,
	whyStopped,
	type LiveCheck,
	type RestartDeadlineSample
} from '$lib/modules/offices/model';
import type { OfficeInfo } from '$lib/proto';

export interface LiveSource {
	organizationId(): string;
	/** The offices saved with the connection: shown until the host has listed them. */
	saved(): OfficeInfo[];
	/** The office this app is connected to. */
	hereId(): string;
	/** Every office the page lists (the host's list with the offices that joined layered beside it). */
	current(): OfficeInfo[];
	/** Offices that joined: HQ reports them, so their saved status is their status. */
	joined(): ReadonlySet<string>;
	/** A Retry that worked removed the button the keyboard was on: where focus goes instead. */
	onback(): void;
}

export class OfficesLive {
	/** The offices as the host last listed them; until it has, `undefined` (the saved ones are shown). */
	reported = $state.raw<OfficeInfo[] | undefined>();
	check = $state<LiveCheck>('checking');
	/** What the host said it can do in its last hello (`office.status_changed`, `office.restart`). */
	capabilities = $state.raw<string[] | undefined>();
	/** True from the first failure until the office answers again: the Retry button stays while it works. */
	retryingProbe = $state(false);
	// Replaced whole on every sample, never mutated: a plain map in raw state.
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- immutable snapshot replaced wholesale in $state.raw
	#deadlineSamples = $state.raw<ReadonlyMap<string, RestartDeadlineSample>>(new Map());
	#countdownClock = $state(Date.now());
	#reportedFor = '';
	// A relevant push or organization change invalidates pending reads. Only successful reads supersede older reads.
	#statusRevision = 0;
	#refreshSequence = 0;
	#successfulRefresh = 0;
	#lastOffice: string | undefined;
	#source: LiveSource;
	#probe;

	constructor(source: LiveSource) {
		this.#source = source;
		this.#probe = createProbe(officeApi, (next, caps) => {
			this.check = next;
			if (next === 'down') this.retryingProbe = true;
			else if (next === 'up') {
				this.retryingProbe = false;
				this.capabilities = caps;
				void this.refresh();
			}
			if (
				next === 'up' &&
				typeof document !== 'undefined' &&
				document.activeElement?.matches('[data-testid=retry-office]')
			)
				source.onback();
		});
	}

	/**
	 * Keeps the live side in step with the page (organization, deadlines, the connected office). Call during the page's
	 * initialization, once everything `source` reads is declared.
	 */
	watch(): void {
		const source = this.#source;
		$effect(() => {
			const id = source.organizationId();
			if (this.#reportedFor !== id) {
				this.#reportedFor = id;
				this.#statusRevision += 1;
				this.reported = undefined;
			}
		});
		// A new deadline gets an immediate sample, which anchors its stable live-status announcement.
		$effect.pre(() => {
			const current = source.current();
			this.#deadlineSamples = sampleRestartDeadlines(
				current,
				untrack(() => this.#deadlineSamples),
				Date.now()
			);
		});
		$effect(() => {
			const deadlines = Array.from(this.#deadlineSamples.values(), ({ deadline }) =>
				Date.parse(deadline)
			);
			let timer: ReturnType<typeof setTimeout> | undefined;
			const tick = () => {
				const now = Date.now();
				this.#countdownClock = now;
				const delay = Math.min(
					...deadlines
						.filter((deadline) => deadline > now)
						.map((deadline) => ((deadline - now) % 10_000) + 1)
				);
				// Cross the exact deadline boundary by 1 ms before displaying the next lower step.
				if (Number.isFinite(delay)) timer = setTimeout(tick, delay);
			};
			tick();
			return () => clearTimeout(timer);
		});
		// Another organization's office is another thing to verify: don't carry the last answer over.
		$effect(() => {
			const id = source.hereId();
			if (this.#lastOffice !== undefined && id !== this.#lastOffice) {
				this.retryingProbe = false;
				void this.#probe.run('explicit');
			}
			this.#lastOffice = id;
		});
	}

	/** The host's list, or the saved one until it has listed them. */
	get listed(): OfficeInfo[] {
		return this.reported ?? this.#source.saved();
	}

	/** Checks now and every half minute, and listens for the host's status pushes. Returns the stop. */
	start(): () => void {
		let stopListening: (() => void) | undefined;
		let leaving = false;
		void officeApi
			.onOfficeStatus((update) => {
				const before = this.listed;
				const next = applyStatusUpdate(before, this.#source.organizationId(), update);
				// An unrelated organization's single-office push returns the same list.
				if (next !== before) this.#statusRevision += 1;
				this.reported = next;
			})
			.then((stop) => {
				if (leaving) stop();
				else stopListening = stop;
			})
			.catch(() => {
				// No live updates then: the check every half minute still lists the statuses.
			});
		void this.#probe.run('explicit');
		const again = setInterval(() => void this.#probe.run('background'), 30_000);
		return () => {
			clearInterval(again);
			leaving = true;
			stopListening?.();
		};
	}

	/** Retry reaching the connected office. */
	retry(): void {
		if (this.check !== 'checking') void this.#probe.run('explicit');
	}

	/**
	 * Lists the statuses the host reports again. Read-only: it remembers nothing and does not change which
	 * organization is open (an unavailable HQ is listed too).
	 */
	refresh = async (): Promise<void> => {
		const id = this.#source.organizationId();
		const revision = this.#statusRevision;
		const request = ++this.#refreshSequence;
		try {
			const found = (await officeApi.peekOrganizations()).find((o) => o.id === id);
			if (
				id === this.#source.organizationId() &&
				revision === this.#statusRevision &&
				request > this.#successfulRefresh
			) {
				// A successful empty result also fences older reads, preserving the absence it reported.
				this.#successfulRefresh = request;
				if (found) this.reported = found.offices;
			}
		} catch {
			// The next tick tries again; the probe already says whether the host answers.
		}
	};

	#office = (officeId: string) => this.#source.current().find((o) => o.id === officeId);

	/** The status to show: what the app has verified for the connected office, HQ's report for the others. */
	statusOf = (officeId: string) =>
		this.#source.joined().has(officeId)
			? (this.#office(officeId)?.status ?? 'unknown')
			: displayStatusFor(
					officeId,
					this.#source.hereId(),
					this.check,
					this.#office(officeId)?.status,
					this.capabilities
				);

	#observedAt(office: OfficeInfo) {
		const sample = this.#deadlineSamples.get(office.id);
		return sample && sample.deadline === office.next_restart_at ? sample.observedAt : undefined;
	}

	#stopped(officeId: string) {
		// HQ reports an office that joined as offline when it is disconnected (§3.1 Offline, "last seen 2 h ago"):
		// only a reason from its owning instance makes it Stopped.
		if (this.#source.joined().has(officeId) && !this.#office(officeId)?.status_reason) return false;
		const shown = this.statusOf(officeId);
		return shown === 'offline' || shown === 'unavailable';
	}

	/** Why a stopped office stopped, for its label: a restart in a form that doesn't tick. */
	labelWhyOf = (officeId: string) => {
		const office = this.#office(officeId);
		// eslint-disable-next-line svelte/prefer-svelte-reactivity -- an instant handed to a pure function
		const at = (ms: number) => new Date(ms);
		return office && this.#stopped(officeId)
			? whyForLabel(office, at(this.#observedAt(office) ?? Date.now()))
			: undefined;
	};

	/** Why a stopped office stopped, as shown: with the countdown. */
	whyOf = (officeId: string) => {
		const office = this.#office(officeId);
		if (!office || !this.#stopped(officeId)) return undefined;
		const at = Math.max(this.#countdownClock, this.#observedAt(office) ?? this.#countdownClock);
		// eslint-disable-next-line svelte/prefer-svelte-reactivity -- an instant handed to a pure function
		return whyStopped(office, new Date(at));
	};
}
