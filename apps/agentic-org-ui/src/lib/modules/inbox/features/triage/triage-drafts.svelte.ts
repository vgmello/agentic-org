import { readInboxCommentDrafts, writeInboxCommentDrafts } from '../../device-comments';
import { deleteDraftKey } from './triage-action-exec';

function draftKey(orgId: string, itemId: string): string {
	return `${orgId}:${itemId}`;
}

export class TriageDraftStore {
	denyDrafts = $state<Record<string, string>>({});
	replyDrafts = $state<Record<string, string>>({});
	commentDrafts = $state<Record<string, string>>(readInboxCommentDrafts());

	getDeny(orgId: string, itemId: string | undefined): string {
		return itemId ? (this.denyDrafts[draftKey(orgId, itemId)] ?? '') : '';
	}

	getReply(orgId: string, itemId: string | undefined): string {
		return itemId ? (this.replyDrafts[draftKey(orgId, itemId)] ?? '') : '';
	}

	getComment(orgId: string, itemId: string | undefined): string {
		return itemId ? (this.commentDrafts[draftKey(orgId, itemId)] ?? '') : '';
	}

	setDeny(orgId: string, itemId: string, value: string): void {
		this.denyDrafts = { ...this.denyDrafts, [draftKey(orgId, itemId)]: value };
	}

	setReply(orgId: string, itemId: string, value: string): void {
		this.replyDrafts = { ...this.replyDrafts, [draftKey(orgId, itemId)]: value };
	}

	setComment(orgId: string, itemId: string, value: string): void {
		const next = { ...this.commentDrafts, [draftKey(orgId, itemId)]: value };
		this.commentDrafts = next;
		writeInboxCommentDrafts(next);
	}

	clearComment(orgId: string, itemId: string): void {
		const next = deleteDraftKey(this.commentDrafts, draftKey(orgId, itemId));
		if (next) {
			this.commentDrafts = next;
			writeInboxCommentDrafts(next);
		}
	}

	clearActionDrafts(orgId: string, itemId: string): void {
		const key = draftKey(orgId, itemId);
		const nextDeny = deleteDraftKey(this.denyDrafts, key);
		if (nextDeny) this.denyDrafts = nextDeny;
		const nextReply = deleteDraftKey(this.replyDrafts, key);
		if (nextReply) this.replyDrafts = nextReply;
	}
}
