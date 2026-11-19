// What Organization settings › Updates reads (server-installation-and-access-ux §5; app-shell-ux §0.1).
// The instance's version is the web API's instance view `server_version`, the release pin's version
// (docs/design/web-api-v1.md §3). The update check and each office's version are not served by the web API yet:
// these shapes are the UI's proposal for that read (see .agent-kit/tasks/v1-ui-updates-contract.md).

/**
 * What HQ last learned about a newer release. Background checks discover and stage updates under the instance's
 * policy; an instance in offline mode makes no checks (organization-data/11 "Release lifecycle"; SRV-34).
 * A state this version doesn't know decodes to `unknown`.
 */
export type UpdateCheck =
	| { state: 'up_to_date'; checkedAt: string }
	| {
			state: 'available';
			version: string;
			checkedAt: string;
			/** Downloaded and verified already, so the upgrade doesn't fetch it again. */
			staged: boolean;
	  }
	| { state: 'failed'; checkedAt: string; reason: string }
	| { state: 'offline' }
	| { state: 'unknown' };

/** One office of the organization and the release it last reported. */
export interface OfficeVersion {
	id: string;
	name: string;
	role: 'hq' | 'satellite';
	/** The machine or instance it runs on, as HQ names it: "This Mac", "build-box.local". */
	host: string;
	/**
	 * The local name of the instance it runs in on that machine (`default` when it has none), or null when HQ doesn't
	 * know it. The upgrade command needs it: `--name` defaults to `default` (server spec §5).
	 */
	instance: string | null;
	/** The release it runs, or null when HQ has not heard it (an office not seen since joining, say). */
	version: string | null;
	/** HQ can't raise the organization's protocol level until this office updates (offices-hq-and-satellites "Versions"). */
	holdsBack: boolean;
}

export interface UpdatesOverview {
	/** The instance that hosts HQ: its local name (`default` when it has none) and release pin's version. */
	instance: { name: string; version: string };
	check: UpdateCheck;
	/** Every office of the organization; HQ always exists. */
	offices: OfficeVersion[];
}
