<script lang="ts">
	import { api } from '$lib/api/client';
	import type { KillmailSummary, PaginatedKillmails } from '$lib/api/client';
	import { formatPrice } from '$lib/utils/formatters';
	import { onMount } from 'svelte';

	let {
		entityType,
		entityId
	}: {
		entityType: 'character' | 'corporation' | 'alliance';
		entityId: number;
	} = $props();

	let activeTab = $state<'kills' | 'losses'>('kills');
	let killsData = $state<PaginatedKillmails | null>(null);
	let lossesData = $state<PaginatedKillmails | null>(null);
	let killsPage = $state(1);
	let lossesPage = $state(1);
	let loading = $state(false);
	let error = $state<string | null>(null);

	async function fetchKills(page: number) {
		if (entityType === 'character') return api.getCharacterKills(entityId, page);
		if (entityType === 'corporation') return api.getCorporationKills(entityId, page);
		return api.getAllianceKills(entityId, page);
	}

	async function fetchLosses(page: number) {
		if (entityType === 'character') return api.getCharacterLosses(entityId, page);
		if (entityType === 'corporation') return api.getCorporationLosses(entityId, page);
		return api.getAllianceLosses(entityId, page);
	}

	async function loadTab(tab: 'kills' | 'losses', page: number) {
		loading = true;
		error = null;
		try {
			if (tab === 'kills') {
				killsData = await fetchKills(page);
				killsPage = page;
			} else {
				lossesData = await fetchLosses(page);
				lossesPage = page;
			}
		} catch (e) {
			console.error(`[nea] ${tab} load failed`, e);
			error = e instanceof Error ? e.message : `Failed to load ${tab}`;
		} finally {
			loading = false;
		}
	}

	onMount(() => {
		loadTab('kills', 1);
		loadTab('losses', 1);
	});

	function switchTab(tab: 'kills' | 'losses') {
		activeTab = tab;
	}

	let currentData = $derived(activeTab === 'kills' ? killsData : lossesData);
	let currentPage = $derived(activeTab === 'kills' ? killsPage : lossesPage);
	let totalPages = $derived(
		currentData ? Math.ceil(currentData.total / currentData.per_page) : 0
	);

	function goPage(page: number) {
		loadTab(activeTab, page);
	}

	function timeAgo(dateStr: string): string {
		const now = Date.now();
		const then = new Date(dateStr).getTime();
		const diffMs = now - then;
		const mins = Math.floor(diffMs / 60000);
		if (mins < 60) return `${mins}m ago`;
		const hours = Math.floor(mins / 60);
		if (hours < 24) return `${hours}h ago`;
		const days = Math.floor(hours / 24);
		if (days < 30) return `${days}d ago`;
		return new Date(dateStr).toLocaleDateString();
	}
</script>

<section>
	<h2 class="mb-4 text-lg font-semibold">Kill / Loss History</h2>
	<div class="mb-4 flex gap-2">
		<button
			onclick={() => switchTab('kills')}
			class="rounded border px-4 py-2 text-sm transition-colors"
			class:border-[var(--color-accent-blue)]={activeTab === 'kills'}
			class:text-[var(--color-accent-blue)]={activeTab === 'kills'}
			class:border-[var(--color-border)]={activeTab !== 'kills'}
			class:text-[var(--color-text-secondary)]={activeTab !== 'kills'}
		>
			Kills{killsData ? ` (${killsData.total})` : ''}
		</button>
		<button
			onclick={() => switchTab('losses')}
			class="rounded border px-4 py-2 text-sm transition-colors"
			class:border-[var(--color-accent-red)]={activeTab === 'losses'}
			class:text-[var(--color-accent-red)]={activeTab === 'losses'}
			class:border-[var(--color-border)]={activeTab !== 'losses'}
			class:text-[var(--color-text-secondary)]={activeTab !== 'losses'}
		>
			Losses{lossesData ? ` (${lossesData.total})` : ''}
		</button>
	</div>

	{#if error}
		<div class="rounded-lg border border-[var(--color-accent-red)] bg-[var(--color-bg-secondary)] p-4 text-sm text-[var(--color-accent-red)]">
			{error}
		</div>
	{:else if loading && !currentData}
		<div class="space-y-2">
			{#each Array(5) as _}
				<div class="h-12 animate-pulse rounded bg-[var(--color-bg-tertiary)]"></div>
			{/each}
		</div>
	{:else if currentData && currentData.killmails.length > 0}
		<div class="overflow-x-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]">
			<table class="w-full text-left text-sm">
				<thead>
					<tr class="border-b border-[var(--color-border)] text-[var(--color-text-secondary)]">
						<th class="px-4 py-3 font-medium">Ship</th>
						<th class="px-4 py-3 font-medium">Victim</th>
						<th class="px-4 py-3 font-medium text-right">ISK Value</th>
						<th class="px-4 py-3 font-medium text-right">Attackers</th>
						<th class="px-4 py-3 font-medium text-right">Time</th>
					</tr>
				</thead>
				<tbody>
					{#each currentData.killmails as km}
						<tr class="border-b border-[var(--color-border)] last:border-b-0 hover:bg-[var(--color-bg-tertiary)] cursor-pointer"
							onclick={() => { window.location.href = `/killmails/${km.killmail_id}`; }}>
							<td class="px-4 py-3">
								<div class="flex items-center gap-2">
									{#if km.victim_ship_type_id}
										<img
											src="https://images.evetech.net/types/{km.victim_ship_type_id}/icon?size=32"
											alt={km.victim_ship_name ?? 'Unknown'}
											class="h-8 w-8"
										/>
									{/if}
									<span class="text-[var(--color-text-primary)]">
										{km.victim_ship_name ?? 'Unknown'}
									</span>
								</div>
							</td>
							<td class="px-4 py-3">
								{#if km.victim_character_name}
									<a
										href="/characters/{km.victim_character_id}"
										class="text-[var(--color-accent-blue)] hover:underline"
										onclick={(e) => e.stopPropagation()}
									>
										{km.victim_character_name}
									</a>
								{:else}
									<span class="text-[var(--color-text-secondary)]">Unknown</span>
								{/if}
							</td>
							<td class="px-4 py-3 text-right font-mono text-[var(--color-text-secondary)]">
								{km.total_value != null ? formatPrice(km.total_value) : '--'}
							</td>
							<td class="px-4 py-3 text-right font-mono text-[var(--color-text-secondary)]">
								{km.attacker_count ?? '--'}
							</td>
							<td class="px-4 py-3 text-right text-[var(--color-text-secondary)]">
								{timeAgo(km.kill_time)}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>

		<!-- Pagination -->
		{#if totalPages > 1}
			<div class="mt-4 flex items-center justify-center gap-4">
				<button
					onclick={() => goPage(currentPage - 1)}
					disabled={currentPage <= 1 || loading}
					class="rounded border border-[var(--color-border)] px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-bg-tertiary)] disabled:opacity-40"
				>
					Prev
				</button>
				<span class="text-sm text-[var(--color-text-secondary)]">
					Page {currentPage} of {totalPages}
				</span>
				<button
					onclick={() => goPage(currentPage + 1)}
					disabled={currentPage >= totalPages || loading}
					class="rounded border border-[var(--color-border)] px-3 py-1.5 text-sm text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-bg-tertiary)] disabled:opacity-40"
				>
					Next
				</button>
			</div>
		{/if}
	{:else if currentData}
		<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-8 text-center text-[var(--color-text-secondary)]">
			No {activeTab} data available.
		</div>
	{/if}
</section>
