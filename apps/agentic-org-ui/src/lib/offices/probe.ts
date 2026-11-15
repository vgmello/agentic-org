// How the Offices screen checks the office this app is connected to (OFC-12): ask it, and trust the answer
// only while it is fresh. Kept apart from the page so the rules can be tested with fake timers.
import type { LiveCheck } from './model';

export interface Office {
	/** Read-only: does the running office answer? Rejects with an `AppError`-shaped error when it doesn't. */
	ping(): Promise<{ capabilities?: string[] } | void>;
	/** Install, register and start the office if needed. Only an explicit Retry may do this. */
	ensureRunning(): Promise<unknown>;
}

export interface ProbeOptions {
	/** How long a background check may take before the office counts as not answering. */
	backgroundMs?: number;
	/** An explicit Retry may start the office, which takes longer. */
	explicitMs?: number;
}

const capabilitiesOf = (hello: { capabilities?: string[] } | void): string[] => (hello && Array.isArray(hello.capabilities) ? hello.capabilities : []);
const codeOf = (e: unknown) => (e && typeof e === 'object' && 'code' in e ? String((e as { code: unknown }).code) : '');

/**
 * `run('background')` is the every-half-minute check: read-only, skipped while another check is running, and it
 * keeps the last answer instead of showing "Checking…". `run('explicit')` is the user's Retry: it shows
 * "Checking…", may start the office when it isn't running, and replaces any check still in flight. The newest
 * check wins: an older, slower one never overwrites it. Silence past the timeout is "down", never "up".
 */
export function createProbe(office: Office, onChange: (check: LiveCheck, capabilities?: string[]) => void, { backgroundMs = 5000, explicitMs = 20_000 }: ProbeOptions = {}) {
	let newest = 0;
	let running = false;

	async function ask(explicit: boolean): Promise<string[] | undefined> {
		try {
			return capabilitiesOf(await office.ping());
		} catch (e) {
			// Nothing started the office yet (the app just opened here), or the user asked to bring it back: start it.
			if (codeOf(e) === 'not_connected' || (explicit && codeOf(e) === 'unreachable')) {
				await office.ensureRunning();
				return capabilitiesOf(await office.ping());
			} else throw e;
		}
	}

	async function run(kind: 'background' | 'explicit'): Promise<void> {
		const explicit = kind === 'explicit';
		if (!explicit && running) return;
		const mine = ++newest;
		running = true;
		if (explicit) onChange('checking');
		let timer: ReturnType<typeof setTimeout> | undefined;
		const silence = new Promise<never>((_, reject) => (timer = setTimeout(() => reject(new Error('no answer')), explicit ? explicitMs : backgroundMs)));
		let result: LiveCheck;
		let capabilities: string[] | undefined;
		try {
			capabilities = await Promise.race([ask(explicit), silence]);
			result = 'up';
		} catch {
			result = 'down';
		} finally {
			clearTimeout(timer);
		}
		if (mine === newest) {
			running = false;
			onChange(result, capabilities);
		}
	}

	return { run };
}
