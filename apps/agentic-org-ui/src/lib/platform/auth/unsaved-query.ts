/**
 * SIGN-19: "any open tab of this HQ" has unsaved text. Every tab answers queries on a
 * same-origin channel from its own draft registry; the asking tab waits a short, bounded
 * time. No text crosses the channel, only a boolean. A tab that doesn't answer in time
 * counts as having none (the answer is a warning, never an authorization).
 */
type Channel = Pick<BroadcastChannel, 'postMessage' | 'addEventListener' | 'removeEventListener'>;
type Options = { channel: Channel; local: () => boolean; timeoutMs?: number; newId?: () => string };

export function createUnsavedQuery({
	channel,
	local,
	timeoutMs = 250,
	newId = () => crypto.randomUUID()
}: Options) {
	const answer: EventListener = (event) => {
		const data: unknown = (event as MessageEvent).data;
		if (!data || typeof data !== 'object') return;
		const message = data as Record<string, unknown>;
		if (message.kind === 'unsaved?' && typeof message.id === 'string') {
			channel.postMessage({ kind: 'unsaved', id: message.id, unsaved: local() });
		}
	};
	channel.addEventListener('message', answer);
	return Object.freeze({
		/** True when this tab or any tab answering within the timeout reports unsaved text. */
		anyUnsaved(): Promise<boolean> {
			if (local()) return Promise.resolve(true);
			const id = newId();
			return new Promise<boolean>((resolve) => {
				const reply: EventListener = (event) => {
					const data: unknown = (event as MessageEvent).data;
					if (!data || typeof data !== 'object') return;
					const message = data as Record<string, unknown>;
					if (message.kind === 'unsaved' && message.id === id && message.unsaved === true)
						finish(true);
				};
				const timer = setTimeout(() => finish(false), timeoutMs);
				function finish(value: boolean) {
					clearTimeout(timer);
					channel.removeEventListener('message', reply);
					resolve(value);
				}
				channel.addEventListener('message', reply);
				channel.postMessage({ kind: 'unsaved?', id });
			});
		},
		dispose() {
			channel.removeEventListener('message', answer);
		}
	});
}
