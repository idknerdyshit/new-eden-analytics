const BASE = '/api';

const log = {
	debug: (msg: string, data?: unknown) => console.debug(`[nea] ${msg}`, data ?? ''),
	info: (msg: string, data?: unknown) => console.info(`[nea] ${msg}`, data ?? ''),
	warn: (msg: string, data?: unknown) => console.warn(`[nea] ${msg}`, data ?? ''),
	error: (msg: string, data?: unknown) => console.error(`[nea] ${msg}`, data ?? '')
};
export { log };

// ── API Response Types ──

export interface SdeType {
	type_id: number;
	name: string;
	group_id: number | null;
	group_name: string | null;
	category_id: number | null;
	category_name: string | null;
	market_group_id: number | null;
	volume: number | null;
	published: boolean;
}

export interface ProductMaterial {
	product_type_id: number;
	product_name: string;
	material_type_id: number;
	material_name: string;
	quantity: number;
}

export interface DashboardData {
	top_correlations: CorrelationResult[];
	top_destruction: DestructionEntry[];
}

export interface Mover {
	type_id: number;
	name: string;
	previous_avg: number;
	current_avg: number;
	change_pct: number;
}

export interface SearchResult {
	items: SdeType[];
	page: number;
	per_page: number;
	total: number;
}

export interface ItemDetail {
	item: SdeType;
	materials: ProductMaterial[];
}

export interface MarketHistoryEntry {
	type_id: number;
	region_id: number;
	date: string;
	average: number;
	highest: number;
	lowest: number;
	volume: number;
	order_count: number;
}

export interface MarketSnapshot {
	type_id: number;
	region_id: number;
	station_id: number | null;
	ts: string;
	best_bid: number | null;
	best_ask: number | null;
	bid_volume: number | null;
	ask_volume: number | null;
	spread: number | null;
}

export interface CorrelationResult {
	id: number;
	product_type_id: number;
	product_name: string;
	material_type_id: number;
	material_name: string;
	lag_days: number;
	correlation_coeff: number;
	granger_f_stat: number | null;
	granger_p_value: number | null;
	granger_significant: boolean;
	window_start: string;
	window_end: string;
	computed_at: string;
}

export interface DestructionEntry {
	type_id: number;
	type_name: string | null;
	date: string;
	quantity_destroyed: number;
	kill_count: number;
}

export interface User {
	character_id: number;
	character_name: string;
}

// ── Character / Profile Types ──

export interface CharacterInfo {
	character_id: number;
	name: string;
	corporation_id: number | null;
	alliance_id: number | null;
	fetched_at: string;
}

export interface CharacterProfile {
	character_id: number;
	total_kills: number;
	total_losses: number;
	solo_kills: number;
	solo_losses: number;
	top_ships_flown: ShipCount[] | null;
	top_ships_lost: ShipCount[] | null;
	common_fits: FittingCluster[] | null;
	active_period: { first_seen: string; last_seen: string } | null;
	computed_at: string;
}

export interface ShipCount {
	type_id: number;
	name: string;
	count: number;
}

export interface FittingModule {
	type_id: number;
	name: string;
	flag: number;
}

export interface FittingCluster {
	ship_type_id: number;
	ship_name: string;
	modules: FittingModule[];
	count: number;
	variant_count: number;
}

export interface CharacterDetail {
	character: CharacterInfo;
	profile: CharacterProfile | null;
}

export interface CharacterSearchResult {
	characters: CharacterInfo[];
	page: number;
	per_page: number;
	total: number;
}

export interface KillmailEntry {
	killmail_id: number;
	kill_time: string;
	solar_system_id: number | null;
	total_value: number | null;
	r2z2_sequence_id: number | null;
}

// ── Corporation / Alliance / Doctrine Types ──

export interface CorporationInfo {
	corporation_id: number;
	name: string;
	alliance_id: number | null;
	member_count: number | null;
	fetched_at: string;
}

export interface AllianceInfo {
	alliance_id: number;
	name: string;
	ticker: string | null;
	fetched_at: string;
}

export interface DoctrineEntry {
	ship_type_id: number;
	ship_name: string;
	canonical_fit: FittingModule[];
	occurrences: number;
	pilot_count: number;
	variant_count: number;
}

export interface ShipUsageEntry {
	type_id: number;
	name: string;
	count: number;
	pct: number;
}

export interface ShipTrend {
	type_id: number;
	name: string;
	current_count: number;
	previous_count: number;
	change_pct: number;
}

export interface FleetComp {
	ships: { type_id: number; name: string }[];
	occurrence_count: number;
}

export interface DoctrineProfileData {
	id: number;
	entity_type: string;
	entity_id: number;
	entity_name: string;
	window_days: number;
	member_count: number;
	total_kills: number;
	total_losses: number;
	ship_usage: ShipUsageEntry[] | null;
	doctrines: DoctrineEntry[] | null;
	ship_trends: ShipTrend[] | null;
	fleet_comps: FleetComp[] | null;
	computed_at: string;
}

export interface CorporationDetail {
	corporation: CorporationInfo;
	profiles: DoctrineProfileData[];
}

export interface AllianceDetail {
	alliance: AllianceInfo;
	profiles: DoctrineProfileData[];
}

export interface CorporationSearchResult {
	corporations: CorporationInfo[];
	page: number;
	per_page: number;
	total: number;
}

export interface AllianceSearchResult {
	alliances: AllianceInfo[];
	page: number;
	per_page: number;
	total: number;
}

// ── Fetch Wrapper ──

class ApiError extends Error {
	status: number;
	constructor(status: number, message: string) {
		super(message);
		this.status = status;
	}
}

async function fetchJson<T>(path: string, init?: RequestInit): Promise<T> {
	log.debug(`fetch ${path}`);
	const start = performance.now();
	const controller = new AbortController();
	const timeout = setTimeout(() => controller.abort(), 30_000);
	try {
		const res = await fetch(`${BASE}${path}`, { ...init, signal: controller.signal });
		const elapsed = Math.round(performance.now() - start);
		const requestId = res.headers.get('x-request-id');
		if (!res.ok) {
			const body = await res.text();
			log.error(`fetch ${path} failed`, { status: res.status, elapsed, requestId, body });
			throw new ApiError(res.status, `API ${res.status}: ${body}`);
		}
		log.debug(`fetch ${path} complete`, { status: res.status, elapsed, requestId });
		return res.json();
	} finally {
		clearTimeout(timeout);
	}
}

export const api = {
	dashboard: () => fetchJson<DashboardData>('/dashboard'),
	movers: () => fetchJson<Mover[]>('/dashboard/movers'),
	searchItems: (q: string, page = 1) =>
		fetchJson<SearchResult>(`/items?q=${encodeURIComponent(q)}&page=${page}`),
	getItem: (typeId: number) => fetchJson<ItemDetail>(`/items/${typeId}`),
	marketHistory: (typeId: number, days = 90) =>
		fetchJson<MarketHistoryEntry[]>(`/market/${typeId}/history?days=${days}`),
	marketSnapshots: (typeId: number, hours = 24) =>
		fetchJson<MarketSnapshot[]>(`/market/${typeId}/snapshots?hours=${hours}`),
	correlations: (typeId: number) =>
		fetchJson<CorrelationResult[]>(`/analysis/${typeId}/correlations`),
	topCorrelations: (limit = 20) =>
		fetchJson<CorrelationResult[]>(`/analysis/top?limit=${limit}`),
	destruction: (typeId: number, days = 90) =>
		fetchJson<DestructionEntry[]>(`/destruction/${typeId}?days=${days}`),
	authMe: async (): Promise<User | null> => {
		try {
			return await fetchJson<User>('/auth/me');
		} catch (e) {
			if (e instanceof ApiError && e.status === 401) {
				return null;
			}
			throw e;
		}
	},
	searchCharacters: (q: string, page = 1) =>
		fetchJson<CharacterSearchResult>(
			`/characters/search?q=${encodeURIComponent(q)}&page=${page}`
		),
	getCharacter: (characterId: number) =>
		fetchJson<CharacterDetail>(`/characters/${characterId}`),
	getCharacterKills: (characterId: number, limit = 20) =>
		fetchJson<KillmailEntry[]>(`/characters/${characterId}/kills?limit=${limit}`),
	getCharacterLosses: (characterId: number, limit = 20) =>
		fetchJson<KillmailEntry[]>(`/characters/${characterId}/losses?limit=${limit}`),
	searchCorporations: (q: string, page = 1) =>
		fetchJson<CorporationSearchResult>(
			`/corporations/search?q=${encodeURIComponent(q)}&page=${page}`
		),
	getCorporation: (corpId: number) =>
		fetchJson<CorporationDetail>(`/corporations/${corpId}`),
	searchAlliances: (q: string, page = 1) =>
		fetchJson<AllianceSearchResult>(
			`/alliances/search?q=${encodeURIComponent(q)}&page=${page}`
		),
	getAlliance: (allianceId: number) =>
		fetchJson<AllianceDetail>(`/alliances/${allianceId}`)
};
