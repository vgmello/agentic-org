export { docsApi, type DocsApi } from './api';
export { docsHref, docHref, newDocHref } from './present';
export { default as DocDiffView } from './components/DocDiffView.svelte';
export { default as DocsTab } from './features/list/DocsTab.svelte';
export { default as DocumentPage } from './features/detail/DocumentPage.svelte';
export type {
	BuiltInDocKind,
	DerivedFromSource,
	DocKindMeta,
	DocSection,
	DocumentBacklink,
	DocumentDraft,
	DocumentLifecycle,
	DocumentScope,
	ProjectDocsOverview,
	ProjectDocumentDetail,
	ProjectDocumentRow
} from './types';
