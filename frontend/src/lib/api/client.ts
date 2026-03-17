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
	material_type_id: number;
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
	date: string;
	quantity_destroyed: number;
	kill_count: number;
}

export interface User {
	character_id: number;
	character_name: string;
	token_expires_at: string | null;
	created_at: string;
	updated_at: string;
}

// ── Fetch Wrapper ──

async function fetchJson<T>(path: string, init?: RequestInit): Promise<T> {
	log.debug(`fetch ${path}`);
	const start = performance.now();
	const res = await fetch(`${BASE}${path}`, init);
	const elapsed = Math.round(performance.now() - start);
	const requestId = res.headers.get('x-request-id');
	if (!res.ok) {
		const body = await res.text();
		log.error(`fetch ${path} failed`, { status: res.status, elapsed, requestId, body });
		throw new Error(`API ${res.status}: ${body}`);
	}
	log.debug(`fetch ${path} complete`, { status: res.status, elapsed, requestId });
	return res.json();
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
	authMe: () => fetchJson<User | null>('/auth/me')
};
