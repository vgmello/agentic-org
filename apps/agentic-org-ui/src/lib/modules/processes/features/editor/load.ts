// What the document editor opens with: the people who may own a document, the places it may be written at, and
// the document being edited or the one an override starts from.
import { processesApi } from '../../api';
import type {
	DocumentDetail,
	DocumentDraft,
	DocumentKind,
	EditorOptions,
	Place
} from '../../types';
import { DOCUMENT_TEMPLATE, TEMPLATES, draftOf, parseKind, parsePlace } from '../../present';

export type EditorMode =
	| { kind: 'new'; place: Place; documentKind: DocumentKind; from?: string; template?: string }
	| { kind: 'edit'; documentId: string; place: Place; documentKind: DocumentKind };

export interface EditorStart {
	options: EditorOptions;
	/** The document being edited. */
	editing?: DocumentDetail;
	/** For an override: the document it replaces, whose text and key it starts from. */
	from?: DocumentDetail;
	draft: DocumentDraft;
}

/** The scope fields of a place documents can be written at; an agent's place writes nothing. */
export function scopeOf(place: Place): Pick<DocumentDraft, 'teamId' | 'projectId'> {
	if (place.kind === 'team') return { teamId: place.id };
	if (place.kind === 'project') return { projectId: place.id };
	return {};
}

export function startingDraft(
	mode: EditorMode,
	from?: DocumentDetail,
	editing?: DocumentDetail
): DocumentDraft {
	if (editing) return draftOf(editing.document);
	const scope = scopeOf(mode.place);
	if (from) {
		const parent = from.document;
		return {
			kind: parent.kind,
			title: parent.title,
			key: parent.key,
			status: 'current',
			ownerId: parent.owner?.id,
			overridable: true,
			...scope,
			bodyMarkdown: parent.bodyMarkdown
		};
	}
	const template = mode.kind === 'new' && mode.template ? TEMPLATES[mode.template] : undefined;
	return {
		kind: mode.documentKind,
		title: template?.title ?? '',
		key: template?.key,
		status: 'current',
		overridable: true,
		...scope,
		bodyMarkdown: template?.bodyMarkdown ?? DOCUMENT_TEMPLATE
	};
}

export async function loadEditor(organizationId: string, mode: EditorMode): Promise<EditorStart> {
	const [options, editing, from] = await Promise.all([
		processesApi.editorOptions(organizationId),
		mode.kind === 'edit' ? processesApi.document(organizationId, mode.documentId) : undefined,
		mode.kind === 'new' && mode.from ? processesApi.document(organizationId, mode.from) : undefined
	]);
	return { options, editing, from, draft: startingDraft(mode, from, editing) };
}

/** The editor's mode from its address: `/documents/new?place=&kind=&from=&template=` or `/documents/<id>?place=&kind=`. */
export function editorMode(url: URL, documentId?: string): EditorMode {
	const place = parsePlace(url.searchParams.get('place'));
	const documentKind = parseKind(url.searchParams.get('kind'));
	if (documentId) return { kind: 'edit', documentId, place, documentKind };
	return {
		kind: 'new',
		place,
		documentKind,
		from: url.searchParams.get('from') ?? undefined,
		template: url.searchParams.get('template') ?? undefined
	};
}
