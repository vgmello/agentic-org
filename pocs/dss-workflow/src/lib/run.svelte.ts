import { guardFlags, type Activation, type Context } from './engine';
import { flow } from './workflow';

/** Simulated run of the flow: the active block, context flags and activation history. */
class FlowRun {
	ctx = $state<Context>(Object.fromEntries(guardFlags(flow.def).map((f) => [f, false])));
	history = $state<Activation[]>([]);
	lastEdge = $state<{ from: string; on: string; to: string } | null>(null);
	error = $state<string | null>(null);

	active = $derived(this.history.at(-1));
	visited = $derived(new Set(this.history.map((a) => a.block.id)));

	command(name: string) {
		this.history = [flow.runCommand(name)];
		this.lastEdge = null;
		this.error = null;
	}

	fire(event: string) {
		const from = this.active?.block.id;
		if (!from) return;
		try {
			const next = flow.fire(from, event, this.ctx);
			this.history = [...this.history, next];
			this.lastEdge = { from, on: event, to: next.block.id };
			this.error = null;
		} catch (e) {
			this.error = (e as Error).message;
		}
	}
}

export const run = new FlowRun();
