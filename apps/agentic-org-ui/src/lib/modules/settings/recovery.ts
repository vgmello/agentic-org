// The Recovery row's words, one per state (server-installation-and-access-ux §4a.6, RCV-4). Pure, so the copy is testable.
import type { RecoveryState } from './types';

/** A message is plain text and code, in order, so the command can be set in mono and copied apart from the sentence. */
export type Segment = string | { code: string };

/** Single quotes, with an embedded quote written `'\''`: names with `"`, `$` or backticks paste safely. */
export const shellQuote = (value: string): string => `'${value.replaceAll("'", "'\\''")}'`;

const SUFFIX_NAME = (instance: string) => (instance && instance !== 'default' ? ` --name ${shellQuote(instance)}` : '');

export const releaseCommand = (organization: string, instance: string, redeliver = false): string =>
	`agentic-org recovery release${redeliver ? ' --redeliver' : ''} --org ${shellQuote(organization)}${SUFFIX_NAME(instance)}`;
export const doctorCommand = (instance: string): string => `agentic-org doctor${SUFFIX_NAME(instance)}`;

/** Said last by every message that reports the code's state (REC-1, RCV-16). */
export const NOT_SUPPORTED = "Recovering an organization after losing the HQ machine isn't supported yet.";

export interface RecoveryContext {
	organization: string;
	hqMachine: string;
	instance: string;
	viewerIsAdministrator: boolean;
	/** Formats an RFC 3339 date in the browser's date format ("10 Nov 2026"). */
	date: (value: string) => string;
}

export interface RecoveryMessage {
	segments: Segment[];
	/** The one command Copy command copies; none when there is nothing to run. */
	command?: string;
}

const on = (when: string | undefined, ctx: RecoveryContext) => (when ? ` on ${ctx.date(when)}` : '');

/** "(shown again 2 times, last on 12 Nov 2026)"; one reads "shown again once, on 12 Nov 2026". */
function again(a: { count: number; lastOn?: string } | undefined, ctx: RecoveryContext): string {
	if (!a || a.count < 1) return '';
	if (a.count === 1) return ` (shown again once${on(a.lastOn, ctx).replace(' on ', ', on ')})`;
	return ` (shown again ${a.count} times${a.lastOn ? `, last on ${ctx.date(a.lastOn)}` : ''})`;
}

export function recoveryMessage(state: RecoveryState, ctx: RecoveryContext): RecoveryMessage {
	const admin = ctx.viewerIsAdministrator ? '' : " This HQ's administrator runs it.";
	switch (state.state) {
		case 'preparing':
			return { segments: ['Recovery code: being prepared. This takes a moment.'] };
		case 'waiting': {
			const command = releaseCommand(ctx.organization, ctx.instance);
			return { segments: ['Recovery code: waiting to be collected on the HQ machine. Run ', { code: command }, ` on ${ctx.hqMachine}.${admin}`], command };
		}
		case 'shown': {
			const command = releaseCommand(ctx.organization, ctx.instance, true);
			return {
				segments: [`Recovery code: shown on ${ctx.hqMachine}${on(state.shownOn, ctx)}${again(state.again, ctx)}, but not confirmed as stored. To show it again and confirm, run `, { code: command }, ` on ${ctx.hqMachine}.${admin}`],
				command
			};
		}
		case 'collected':
			if (state.again && state.again.count > 0) return { segments: [`Recovery code shown${on(state.shownOn, ctx)}${again(state.again, ctx)} · collected${on(state.collectedOn, ctx)}.`] };
			return { segments: [`Recovery code collected${on(state.collectedOn, ctx)}.`] };
		case 'failed': {
			const command = doctorCommand(ctx.instance);
			return { segments: [`Recovery code: couldn't be prepared. ${state.reason.replace(/[.]+$/, '')}. On ${ctx.hqMachine}, run `, { code: command }, '.'], command };
		}
		default:
			return { segments: ['Recovery code: status unknown.'] };
	}
}

export const messageText = (m: RecoveryMessage): string => m.segments.map((s) => (typeof s === 'string' ? s : `\`${s.code}\``)).join('');
