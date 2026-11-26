import type { OrgSnapshot } from '$lib/proto';
import type { ChartEmployee } from './types';

export type OrgCanvasMode = 'teams' | 'people';

export interface CanvasNodeData {
	id: string;
	kind: 'team' | 'person' | 'agent';
	title: string;
	subtitle: string;
	badge?: string;
	badgeTone?: 'accent' | 'ok' | 'warn' | 'muted';
	meta: string;
	avatarName: string;
	href: string;
	parentId?: string;
	childCount: number;
	totalDescendants: number;
	leadId?: string;
	teamId?: string;
	employee?: ChartEmployee;
	memberIds?: string[];
}

export interface PositionedNode {
	data: CanvasNodeData;
	x: number;
	y: number;
	w: number;
	h: number;
	collapsed: boolean;
	hiddenCount: number;
}

export interface CanvasEdge {
	id: string;
	fromId: string;
	toId: string;
	x1: number;
	y1: number;
	x2: number;
	y2: number;
}

export interface CanvasLayoutResult {
	nodes: PositionedNode[];
	edges: CanvasEdge[];
	width: number;
	height: number;
	byId: Map<string, CanvasNodeData>;
}

const NODE_W = 228;
const NODE_H = 88;
const H_GAP = 28;
const V_GAP = 64;
const PAD = 48;

interface RawTreeNode {
	data: CanvasNodeData;
	children: RawTreeNode[];
}

function countDescendants(node: RawTreeNode): number {
	let total = 0;
	for (const child of node.children) {
		total += 1 + countDescendants(child);
	}
	node.data.childCount = node.children.length;
	node.data.totalDescendants = total;
	return total;
}

function buildForest(items: CanvasNodeData[]): {
	roots: RawTreeNode[];
	byId: Map<string, CanvasNodeData>;
} {
	const byId = new Map<string, CanvasNodeData>();
	const wrapped = new Map<string, RawTreeNode>();
	for (const item of items) {
		byId.set(item.id, item);
		wrapped.set(item.id, { data: item, children: [] });
	}

	const roots: RawTreeNode[] = [];
	for (const node of wrapped.values()) {
		const pid = node.data.parentId;
		const parent = pid && pid !== node.data.id ? wrapped.get(pid) : undefined;
		if (parent && !wouldCycle(node.data.id, pid!, byId)) {
			parent.children.push(node);
		} else {
			node.data.parentId = undefined;
			roots.push(node);
		}
	}

	const sortFn = (a: RawTreeNode, b: RawTreeNode) =>
		a.data.title.localeCompare(b.data.title, undefined, { sensitivity: 'base' });
	const sortTree = (list: RawTreeNode[]) => {
		list.sort(sortFn);
		for (const item of list) sortTree(item.children);
	};
	sortTree(roots);
	for (const r of roots) countDescendants(r);
	return { roots, byId };
}

function wouldCycle(startId: string, parentId: string, byId: Map<string, CanvasNodeData>): boolean {
	let cur: string | undefined = parentId;
	const seen = new Set<string>([startId]);
	while (cur) {
		if (seen.has(cur)) return true;
		seen.add(cur);
		cur = byId.get(cur)?.parentId;
	}
	return false;
}

function formatTeamMeta(teamMembers: readonly ChartEmployee[]): string {
	const agentsCount = teamMembers.filter((m) => m.kind === 'agent').length;
	const peopleCount = teamMembers.length - agentsCount;
	const parts: string[] = [];
	if (peopleCount > 0) parts.push(`${peopleCount} ${peopleCount === 1 ? 'person' : 'people'}`);
	if (agentsCount > 0) parts.push(`${agentsCount} ${agentsCount === 1 ? 'agent' : 'agents'}`);
	return parts.length > 0 ? parts.join(' · ') : 'No members yet';
}

function indexEmployeesByTeam(employees: readonly ChartEmployee[]): Map<string, ChartEmployee[]> {
	const map = new Map<string, ChartEmployee[]>();
	for (const emp of employees) {
		const tid = emp.homeTeam?.id;
		if (!tid) continue;
		const list = map.get(tid) ?? [];
		list.push(emp);
		map.set(tid, list);
	}
	return map;
}

function resolveTeamParentId(
	team: { id: string; owner_team_id?: string },
	lead: ChartEmployee | undefined,
	memberById: Map<string, ChartEmployee>
): string | undefined {
	if (team.owner_team_id) return team.owner_team_id;
	const managerTeam = lead?.reportsToId
		? memberById.get(lead.reportsToId)?.homeTeam?.id
		: undefined;
	return managerTeam && managerTeam !== team.id ? managerTeam : undefined;
}

export function buildTeamCanvasItems(
	snapshot: OrgSnapshot | null,
	employees: readonly ChartEmployee[]
): CanvasNodeData[] {
	const teams = snapshot?.teams ?? [];
	const memberById = new Map(employees.map((e) => [e.id, e]));
	const membersByTeam = indexEmployeesByTeam(employees);

	return teams.map((t): CanvasNodeData => {
		const teamMembers = membersByTeam.get(t.id) ?? [];
		const lead = t.lead_member_id ? memberById.get(t.lead_member_id) : undefined;
		return {
			id: t.id,
			kind: 'team',
			title: t.name,
			subtitle: lead ? `Led by ${lead.name}` : 'No team lead',
			badge: `${teamMembers.length}`,
			badgeTone: teamMembers.length > 0 ? 'accent' : 'muted',
			meta: formatTeamMeta(teamMembers),
			avatarName: lead?.name ?? t.name,
			href: `/teams/${encodeURIComponent(t.id)}`,
			parentId: resolveTeamParentId(t, lead, memberById),
			childCount: 0,
			totalDescendants: 0,
			leadId: lead?.id,
			teamId: t.id,
			memberIds: teamMembers.map((m) => m.id)
		};
	});
}

export function buildPeopleCanvasItems(
	employees: readonly ChartEmployee[],
	ownerId: string
): CanvasNodeData[] {
	return employees.map((emp): CanvasNodeData => {
		const isOwner = emp.id === ownerId;
		const badge = isOwner ? 'Owner' : emp.kind === 'agent' ? 'Agent' : 'Person';
		const badgeTone = isOwner ? 'accent' : emp.kind === 'agent' ? 'ok' : 'muted';
		const leadSummary =
			emp.leads.length > 0 ? `Leads ${emp.leads.map((l) => l.name).join(', ')}` : undefined;
		return {
			id: emp.id,
			kind: emp.kind,
			title: emp.name,
			subtitle: `@${emp.handle}`,
			badge,
			badgeTone,
			meta: leadSummary ?? emp.homeTeam?.name ?? 'No home team',
			avatarName: emp.name,
			href: `/members/${encodeURIComponent(emp.id)}`,
			parentId: emp.reportsToId,
			childCount: 0,
			totalDescendants: 0,
			teamId: emp.homeTeam?.id,
			employee: emp
		};
	});
}

function measureSubtreeWidth(node: RawTreeNode, collapsed: ReadonlySet<string>): number {
	if (node.children.length === 0 || collapsed.has(node.data.id)) return NODE_W;
	let sum = 0;
	for (let i = 0; i < node.children.length; i++) {
		if (i > 0) sum += H_GAP;
		sum += measureSubtreeWidth(node.children[i], collapsed);
	}
	return Math.max(NODE_W, sum);
}

export function layoutCanvasTree(
	items: CanvasNodeData[],
	collapsed: ReadonlySet<string>
): CanvasLayoutResult {
	const { roots, byId } = buildForest(items);
	const nodes: PositionedNode[] = [];
	const edges: CanvasEdge[] = [];

	let maxX = PAD * 2 + NODE_W;
	let maxY = PAD * 2 + NODE_H;

	const place = (node: RawTreeNode, leftX: number, topY: number): { cx: number; width: number } => {
		const isCollapsed = collapsed.has(node.data.id) && node.children.length > 0;
		const subW = measureSubtreeWidth(node, collapsed);
		const x = leftX + (subW - NODE_W) / 2;
		const y = topY;

		nodes.push({
			data: node.data,
			x,
			y,
			w: NODE_W,
			h: NODE_H,
			collapsed: isCollapsed,
			hiddenCount: isCollapsed ? node.data.totalDescendants : 0
		});
		maxX = Math.max(maxX, x + NODE_W + PAD);
		maxY = Math.max(maxY, y + NODE_H + PAD);

		if (!isCollapsed && node.children.length > 0) {
			let childX = leftX;
			const childY = topY + NODE_H + V_GAP;
			const totalChildrenW =
				node.children.reduce((acc, c) => acc + measureSubtreeWidth(c, collapsed), 0) +
				H_GAP * (node.children.length - 1);
			if (totalChildrenW < subW) {
				childX += (subW - totalChildrenW) / 2;
			}
			for (const child of node.children) {
				const res = place(child, childX, childY);
				edges.push({
					id: `${node.data.id}->${child.data.id}`,
					fromId: node.data.id,
					toId: child.data.id,
					x1: x + NODE_W / 2,
					y1: y + NODE_H,
					x2: res.cx,
					y2: childY
				});
				childX += res.width + H_GAP;
			}
		}

		return { cx: x + NODE_W / 2, width: subW };
	};

	const isFlatForest = roots.length > 4 && roots.every((r) => r.children.length === 0);
	if (isFlatForest) {
		const cols = Math.min(4, Math.ceil(Math.sqrt(roots.length)));
		for (let i = 0; i < roots.length; i++) {
			const col = i % cols;
			const row = Math.floor(i / cols);
			place(roots[i], PAD + col * (NODE_W + H_GAP), PAD + row * (NODE_H + 28));
		}
		return {
			nodes,
			edges,
			width: Math.max(maxX, PAD * 2 + cols * NODE_W + (cols - 1) * H_GAP),
			height: maxY,
			byId
		};
	}

	let cursorX = PAD;
	for (const root of roots) {
		const res = place(root, cursorX, PAD);
		cursorX += res.width + H_GAP * 1.5;
	}

	return {
		nodes,
		edges,
		width: Math.max(maxX, cursorX + PAD),
		height: maxY,
		byId
	};
}

export function hotPathIds(byId: Map<string, CanvasNodeData>, selectedId?: string): Set<string> {
	const hot = new Set<string>();
	let cur = selectedId;
	while (cur && !hot.has(cur)) {
		hot.add(cur);
		cur = byId.get(cur)?.parentId;
	}
	return hot;
}
