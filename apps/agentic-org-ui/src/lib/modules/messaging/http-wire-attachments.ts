import { decodeAttachmentMetaList, type AttachmentMeta } from '$lib/platform/http/attachments';

export interface MessageAttachmentFilter {
	has_image?: boolean;
	has_file?: boolean;
}

/** Decodes wire `attachments: AttachmentMeta[]` on messages, ask answers, and chat inbox items. */
export function decodeMessageAttachments(raw: unknown): AttachmentMeta[] | undefined {
	const decoded = decodeAttachmentMetaList(raw);
	return decoded.length ? decoded : undefined;
}

/** Serializes `attachment_ids` only when non-empty (matches `skip_serializing_if = "Vec::is_empty"`). */
export function attachmentIdsField(ids: readonly string[] | undefined): {
	attachment_ids?: string[];
} {
	if (!ids?.length) return {};
	const seen = new Set<string>();
	const unique: string[] = [];
	for (const id of ids) {
		if (typeof id === 'string' && id && !seen.has(id)) {
			seen.add(id);
			unique.push(id);
		}
	}
	return unique.length ? { attachment_ids: unique } : {};
}

/** Builds optional `has_image` / `has_file` wire filter fields for `chat/messages/list` and `chat/inbox`. */
export function attachmentFilterFields(filter?: MessageAttachmentFilter): MessageAttachmentFilter {
	if (!filter) return {};
	return {
		...(typeof filter.has_image === 'boolean' ? { has_image: filter.has_image } : {}),
		...(typeof filter.has_file === 'boolean' ? { has_file: filter.has_file } : {})
	};
}

/** Matches `org_proto::attachments::has_content`: a message may carry text, files, or both. */
export function hasMessageContent(
	body: string,
	attachmentIds?: readonly string[],
	legacyAttachments?: readonly string[]
): boolean {
	return (
		body.trim().length > 0 ||
		(attachmentIds !== undefined && attachmentIds.length > 0) ||
		(legacyAttachments !== undefined && legacyAttachments.length > 0)
	);
}
