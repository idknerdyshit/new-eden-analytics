import { writable } from 'svelte/store';
import type { SearchResult } from '$lib/api/client';

export const searchQuery = writable('');
export const searchResults = writable<SearchResult | null>(null);
