// What the profile frame shares with its tabs: the loaded profile, the organization it belongs to, the header's
// actions (so a tab's Pause or Assign task opens the same dialog as the header) and a change counter the tabs
// reload on after an action changes the member.
import { createContext } from 'svelte';
import type { OrgSnapshot } from '$lib/proto';
import type { MemberProfile } from '../../types';

export type ProfileAction = 'run' | 'assign' | 'pause' | 'resume' | 'offboard' | 'edit' | 'move';

export interface ProfileContext {
	readonly organizationId: string;
	readonly organizationName: string;
	readonly profile: MemberProfile;
	readonly snapshot: OrgSnapshot;
	/** Increases after every change made from the profile; tabs reload their reads on it. */
	readonly changes: number;
	/** Opens an action's dialog; `opener` gets focus back when it closes. */
	act(action: ProfileAction, opener?: HTMLElement | null): void;
	/** Reload the profile and the organization after a change. */
	changed(): Promise<void>;
}

export const [profileContext, setProfileContext] = createContext<ProfileContext>();
