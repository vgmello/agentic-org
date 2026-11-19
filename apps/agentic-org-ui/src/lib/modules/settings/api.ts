import { unavailableCall } from '$lib/platform/unavailable-api';
import type {
	AccessOverview,
	CostPeriod,
	CostSummary,
	GeneralSettings,
	IssuedLink,
	NewPriceRate,
	PriceRate
} from './types';

/** What the Organization settings screens (General, Members and access, Costs) read and change. */
export interface SettingsApi {
	general(organizationId: string): Promise<GeneralSettings>;
	/** Makes `memberId`'s account the administrator, confirmed with the administrator's own password. */
	handOverAdministrator(
		organizationId: string,
		memberId: string,
		password: string
	): Promise<GeneralSettings>;

	access(organizationId: string): Promise<AccessOverview>;
	/** A one-time registration link (no account yet) or set-new-password link (has an account). Replaces the previous one. */
	issueLink(
		organizationId: string,
		memberId: string,
		kind: IssuedLink['kind']
	): Promise<IssuedLink>;
	removeAccess(organizationId: string, memberId: string): Promise<AccessOverview>;

	prices(organizationId: string): Promise<PriceRate[]>;
	addPrice(organizationId: string, rate: NewPriceRate): Promise<PriceRate[]>;
	/** The owner's organization cost summary; refuses anyone else with `not_authorized`. */
	costSummary(organizationId: string, period: CostPeriod): Promise<CostSummary>;
}

// The web API does not serve these yet: every call refuses, and each screen says so.
// Under `bun run dev:mock`, e2e/fakes/modules/settings.ts stands in for this file.
export const settingsApi: SettingsApi = {
	general: unavailableCall,
	handOverAdministrator: unavailableCall,
	access: unavailableCall,
	issueLink: unavailableCall,
	removeAccess: unavailableCall,
	prices: unavailableCall,
	addPrice: unavailableCall,
	costSummary: unavailableCall
};
