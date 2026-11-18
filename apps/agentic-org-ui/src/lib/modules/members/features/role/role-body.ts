// A role's Markdown body as plain blocks (roles-ux ROLE-1: Mission, Responsibilities, Working style, Boundaries are
// `##` sections). Only headings, bullet lists and paragraphs are recognised; everything else stays text. Rendered as
// elements, never as HTML.
export type RoleBlock =
	| { kind: 'heading'; text: string }
	| { kind: 'list'; items: string[] }
	| { kind: 'paragraph'; text: string };

export function roleBlocks(body: string): RoleBlock[] {
	const blocks: RoleBlock[] = [];
	let paragraph: string[] = [];
	const flush = () => {
		if (paragraph.length) blocks.push({ kind: 'paragraph', text: paragraph.join(' ') });
		paragraph = [];
	};
	for (const raw of body.split('\n')) {
		const line = raw.trim();
		const heading = /^#{1,6}\s+(.*)$/.exec(line);
		const item = /^[-*]\s+(.*)$/.exec(line);
		if (!line) flush();
		else if (heading) {
			flush();
			blocks.push({ kind: 'heading', text: heading[1] });
		} else if (item) {
			flush();
			const last = blocks.at(-1);
			if (last?.kind === 'list') last.items.push(item[1]);
			else blocks.push({ kind: 'list', items: [item[1]] });
		} else paragraph.push(line);
	}
	flush();
	return blocks;
}
