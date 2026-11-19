// Keyboard tree navigation for the reporting tree (TEAM-10), as in the WAI-ARIA tree pattern, between the nodes'
// profile links: Up and Down move through what is shown, Right opens a collapsed manager or goes to their first
// report, Left closes an open manager or goes to the manager, Home and End go to the first and last node shown.
import { visibleOrder, type ReportingTree } from './tree';

export type TreeMove = { focus: string } | { expand: string } | { collapse: string } | undefined;

export const TREE_KEYS = ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Home', 'End'];

export function treeMove(
	key: string,
	currentId: string,
	tree: ReportingTree,
	collapsed: ReadonlySet<string>
): TreeMove {
	const shown = visibleOrder(tree, collapsed);
	const i = shown.findIndex((n) => n.employee.id === currentId);
	const node = shown[i];
	if (!node) return undefined;
	const at = (j: number) => {
		const target = shown[j];
		return target && target !== node ? { focus: target.employee.id } : undefined;
	};
	const open = node.children.length > 0 && !collapsed.has(currentId);
	switch (key) {
		case 'ArrowDown':
			return at(i + 1);
		case 'ArrowUp':
			return at(i - 1);
		case 'Home':
			return at(0);
		case 'End':
			return at(shown.length - 1);
		case 'ArrowRight':
			if (node.children.length === 0) return undefined;
			return open ? { focus: node.children[0].employee.id } : { expand: currentId };
		case 'ArrowLeft':
			if (open) return { collapse: currentId };
			return node.parentId ? { focus: node.parentId } : undefined;
		default:
			return undefined;
	}
}
