// Project frame context shared by project tabs across domain modules (projects, releases, etc.)
// without introducing a `releases -> projects` cycle.
import { createContext, type Snippet } from 'svelte';
import type { StaleCopy } from '$lib/proto';

export interface ProjectFrameProject {
	readonly id: string;
	readonly name: string;
	readonly slug: string;
	readonly number: number;
	readonly archived: boolean;
	readonly viewer: { readonly canEdit: boolean };
	readonly stale?: StaleCopy;
}

export interface ProjectFrameContext<
	TProject extends ProjectFrameProject = ProjectFrameProject
> {
	readonly organizationId: string;
	readonly projectId: string;
	/** The project as the frame last read it; tabs render only once it has loaded. */
	readonly project: TProject;
	/** Re-reads the project (after a change made in a tab), keeping what is shown until the answer arrives. */
	reload(): Promise<void>;
	/** Optional view-specific controls rendered inside the project toolbar (e.g. Project Activity). */
	setToolbarControls(controls: Snippet | undefined): void;
}

const [rawProjectFrame, rawSetProjectFrame] = createContext<ProjectFrameContext>();

export function projectFrame<
	TProject extends ProjectFrameProject = ProjectFrameProject
>(): ProjectFrameContext<TProject> {
	return rawProjectFrame() as ProjectFrameContext<TProject>;
}

export function setProjectFrame<TProject extends ProjectFrameProject>(
	ctx: ProjectFrameContext<TProject>
): ProjectFrameContext<TProject> {
	return rawSetProjectFrame(ctx) as ProjectFrameContext<TProject>;
}
