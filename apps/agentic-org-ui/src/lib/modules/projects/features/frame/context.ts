// What a project's tabs read from the frame around them: which organization and project, the project as last read,
// and a way to read it again after a change. Set by ProjectFrame; each tab's route renders its tab without props.
import { createContext, type Snippet } from 'svelte';
import type { ProjectDetail } from '../../types';

export interface ProjectFrameContext {
	readonly organizationId: string;
	readonly projectId: string;
	/** The project as the frame last read it; tabs render only once it has loaded. */
	readonly project: ProjectDetail;
	/** Re-reads the project (after a change made in a tab), keeping what is shown until the answer arrives. */
	reload(): Promise<void>;
	/** Optional view-specific controls rendered inside the project toolbar (e.g. Project Activity). */
	setToolbarControls(controls: Snippet | undefined): void;
}

export const [projectFrame, setProjectFrame] = createContext<ProjectFrameContext>();
