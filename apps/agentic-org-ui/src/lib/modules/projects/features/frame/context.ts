// What a project's tabs read from the frame around them: which organization and project, the project as last read,
// and a way to read it again after a change. Set by ProjectFrame; each tab's route renders its tab without props.
import {
	projectFrame as platformProjectFrame,
	setProjectFrame as platformSetProjectFrame,
	type ProjectFrameContext as PlatformProjectFrameContext
} from '$lib/platform/shell/project-frame';
import type { ProjectDetail } from '../../types';

export type ProjectFrameContext = PlatformProjectFrameContext<ProjectDetail>;

export function projectFrame(): ProjectFrameContext {
	return platformProjectFrame<ProjectDetail>();
}

export function setProjectFrame(ctx: ProjectFrameContext): ProjectFrameContext {
	return platformSetProjectFrame(ctx);
}
