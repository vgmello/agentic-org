export interface DraftText {
	read(): string;
	write(text: string): void;
	/** Capture the current draft so a later success cannot erase newer text. */
	clearOnSuccess(): () => void;
}

export interface ThreadDrafts {
	reply: DraftText;
	ask(askId: string): DraftText;
}

export interface DraftStore {
	forThread(threadId: string): ThreadDrafts;
	syncScope(): void;
	dispose(): void;
}

type DraftEntry = { text: string };
type Realm = { organizationId: string; entries: Map<string, DraftEntry> };

export function createDraftStore(currentOrganizationId: () => string, capacity = 100): DraftStore {
	let realm: Realm | undefined;
	let disposed = false;

	function currentRealm() {
		if (disposed) return;
		const organizationId = currentOrganizationId();
		if (realm?.organizationId !== organizationId) {
			realm?.entries.clear();
			realm = { organizationId, entries: new Map() };
		}
		return realm;
	}

	function draft(scope: Realm | undefined, key: string): DraftText {
		const entries = () => currentRealm() === scope ? scope?.entries : undefined;
		return {
			read: () => entries()?.get(key)?.text ?? '',
			write(text) {
				const values = entries();
				if (!values || values.get(key)?.text === text) return;
				values.delete(key);
				if (text === '') return;
				values.set(key, { text });
				if (values.size > capacity) {
					const oldest = values.keys().next().value;
					if (oldest !== undefined) values.delete(oldest);
				}
			},
			clearOnSuccess() {
				const submitted = entries()?.get(key);
				return () => {
					const values = entries();
					if (submitted && values?.get(key) === submitted) values.delete(key);
				};
			}
		};
	}

	return {
		forThread(threadId) {
			const scope = currentRealm();
			return {
				reply: draft(scope, JSON.stringify(['thread', threadId])),
				ask: (askId) => draft(scope, JSON.stringify(['ask', threadId, askId]))
			};
		},
		syncScope() { currentRealm(); },
		dispose() {
			disposed = true;
			realm?.entries.clear();
			realm = undefined;
		}
	};
}
