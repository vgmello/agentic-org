import { getContext, setContext } from 'svelte';

const ATTACHMENT_ORG_KEY = Symbol('Attachment organization id');

/** Provides the active organization ID to descendant attachment primitives without prop-drilling. */
export function provideAttachmentOrgId(getOrganizationId: () => string): void {
	setContext(ATTACHMENT_ORG_KEY, getOrganizationId);
}

/** Reads the ambient organization ID provided by the enclosing workspace/route, if any. */
export function useAttachmentOrgId(): (() => string) | undefined {
	return getContext<(() => string) | undefined>(ATTACHMENT_ORG_KEY);
}
