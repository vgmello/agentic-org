import { unavailableCall } from '$lib/platform/unavailable-api';
import type {
	DerivedFromSource,
	DocumentDraft,
	ProjectDocsOverview,
	ProjectDocumentDetail
} from './types';

/**
 * What the Project Docs screens read and change (decisions-and-tech-debt-ux §0–§6, organization-data 18).
 * Every change is an accepted organization-Git operation against `expectedRevision`; conflicts reject with
 * `code: 'conflict'` and carry `latest` so the editor preserves unsaved text and shows a comparison.
 */
export interface DocsApi {
	list(organizationId: string, projectId: string): Promise<ProjectDocsOverview>;
	get(organizationId: string, projectId: string, docId: string): Promise<ProjectDocumentDetail>;
	create(organizationId: string, projectId: string, draft: DocumentDraft): Promise<{ id: string }>;
	save(
		organizationId: string,
		projectId: string,
		docId: string,
		change: { draft: DocumentDraft; expectedRevision: number; summary?: string }
	): Promise<ProjectDocumentDetail>;
	issueVersion(
		organizationId: string,
		projectId: string,
		docId: string,
		change: {
			note: string;
			derivedFrom: Pick<DerivedFromSource, 'documentId' | 'version'>[];
			expectedRevision: number;
		}
	): Promise<ProjectDocumentDetail>;
	addSection(
		organizationId: string,
		projectId: string,
		section: { path: string; title: string; summary: string }
	): Promise<void>;
	renameSection(
		organizationId: string,
		projectId: string,
		sectionPath: string,
		change: { title: string; summary: string }
	): Promise<void>;
	reorderSections(organizationId: string, projectId: string, orderedPaths: string[]): Promise<void>;
	moveDocument(
		organizationId: string,
		projectId: string,
		docId: string,
		change: { sectionPath: string; fileName?: string; expectedRevision: number }
	): Promise<void>;
}

// Not served by the web API yet: every call refuses cleanly so production builds never show mock data.
// Under `bun run dev:mock`, e2e/fakes/modules/docs.ts stands in for this file.
export const docsApi: DocsApi = {
	list: unavailableCall,
	get: unavailableCall,
	create: unavailableCall,
	save: unavailableCall,
	issueVersion: unavailableCall,
	addSection: unavailableCall,
	renameSection: unavailableCall,
	reorderSections: unavailableCall,
	moveDocument: unavailableCall
};
