import type { StagedAttachment } from '$lib/components/attachments/model';
import { subscribeAttachmentsSessionEnded } from '$lib/platform/http/attachments-runtime';

function revokeStagedPreviews(items: readonly StagedAttachment[]): void {
	for (const item of items) {
		if (item.previewUrl) URL.revokeObjectURL(item.previewUrl);
	}
}

export interface DraftText {
	read(): string;
	write(text: string): void;
	readAttachments?(): StagedAttachment[];
	writeAttachments?(items: StagedAttachment[]): void;
	/** Capture the current draft so a later success cannot erase newer text or attachments. */
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

type DraftEntry = { text: string; attachments: StagedAttachment[] };
type Realm = { organizationId: string; entries: Map<string, DraftEntry> };

function clearRealmEntries(entries: Map<string, DraftEntry>) {
	for (const entry of entries.values()) {
		if (entry.attachments.length) revokeStagedPreviews(entry.attachments);
	}
	entries.clear();
}

export function createDraftStore(
	currentOrganizationId: () => string,
	capacity = 100,
	onSessionEnded: (listener: () => void) => () => void = subscribeAttachmentsSessionEnded
): DraftStore {
	let realm: Realm | undefined;
	let disposed = false;

	const unsubscribeSession = onSessionEnded(() => {
		if (realm) clearRealmEntries(realm.entries);
		realm = undefined;
	});

	function currentRealm() {
		if (disposed) return;
		const organizationId = currentOrganizationId();
		if (realm?.organizationId !== organizationId) {
			if (realm) clearRealmEntries(realm.entries);
			realm = { organizationId, entries: new Map() };
		}
		return realm;
	}

	function trimCapacity(values: Map<string, DraftEntry>) {
		while (values.size > capacity) {
			const oldestKey = values.keys().next().value;
			if (oldestKey === undefined) break;
			const evicted = values.get(oldestKey);
			if (evicted?.attachments.length) revokeStagedPreviews(evicted.attachments);
			values.delete(oldestKey);
		}
	}

	function draft(scope: Realm | undefined, key: string): DraftText {
		const entries = () => (currentRealm() === scope ? scope?.entries : undefined);
		const update = (nextText: string, nextAttachments: StagedAttachment[]) => {
			const values = entries();
			if (!values) return;
			values.delete(key);
			if (nextText === '' && nextAttachments.length === 0) return;
			values.set(key, { text: nextText, attachments: nextAttachments });
			trimCapacity(values);
		};
		return {
			read: () => entries()?.get(key)?.text ?? '',
			write(text) {
				const values = entries();
				const existing = values?.get(key);
				if (!values || (existing?.text ?? '') === text) return;
				update(text, existing?.attachments ?? []);
			},
			readAttachments: () => entries()?.get(key)?.attachments ?? [],
			writeAttachments(items) {
				const values = entries();
				const existing = values?.get(key);
				if (!values || (existing?.attachments ?? []) === items) return;
				update(existing?.text ?? '', items);
			},
			clearOnSuccess() {
				const submitted = entries()?.get(key);
				return () => {
					const values = entries();
					if (submitted && values?.get(key) === submitted) {
						if (submitted.attachments.length) revokeStagedPreviews(submitted.attachments);
						values.delete(key);
					}
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
		syncScope() {
			currentRealm();
		},
		dispose() {
			disposed = true;
			unsubscribeSession();
			if (realm) clearRealmEntries(realm.entries);
			realm = undefined;
		}
	};
}
