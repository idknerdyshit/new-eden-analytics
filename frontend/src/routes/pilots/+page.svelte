<script lang="ts">
	import { api } from '$lib/api/client';
	import type { CharacterInfo, CorporationInfo, AllianceInfo } from '$lib/api/client';

	let activeSearchTab = $state<'pilots' | 'corporations' | 'alliances'>('pilots');
	let query = $state('');
	let currentPage = $state(1);
	let perPage = $state(20);
	let totalResults = $state(0);
	let loading = $state(false);
	let searched = $state(false);
	let error = $state<string | null>(null);

	let pilotResults = $state<CharacterInfo[]>([]);
	let corpResults = $state<CorporationInfo[]>([]);
	let allianceResults = $state<AllianceInfo[]>([]);

	let debounceTimer: ReturnType<typeof setTimeout> | undefined;

	function onInput() {
		currentPage = 1;
		clearTimeout(debounceTimer);
		debounceTimer = setTimeout(() => {
			performSearch();
		}, 300);
	}

	function switchTab(tab: 'pilots' | 'corporations' | 'alliances') {
		activeSearchTab = tab;
		currentPage = 1;
		pilotResults = [];
		corpResults = [];
		allianceResults = [];
		totalResults = 0;
		searched = false;
		if (query.trim()) {
			performSearch();
		}
	}

	async function performSearch() {
		const q = query.trim();
		if (!q) {
			pilotResults = [];
			corpResults = [];
			allianceResults = [];
			searched = false;
			return;
		}

		loading = true;
		searched = true;
		error = null;
		try {
			if (activeSearchTab === 'pilots') {
				const data = await api.searchCharacters(q, currentPage);
				pilotResults = data.characters;
				perPage = data.per_page;
				totalResults = data.total;
			} else if (activeSearchTab === 'corporations') {
				const data = await api.searchCorporations(q, currentPage);
				corpResults = data.corporations;
				perPage = data.per_page;
				totalResults = data.total;
			} else {
				const data = await api.searchAlliances(q, currentPage);
				allianceResults = data.alliances;
				perPage = data.per_page;
				totalResults = data.total;
			}
		} catch (e) {
			console.error('[nea] search failed', e);
			error = e instanceof Error ? e.message : 'Search failed';
			pilotResults = [];
			corpResults = [];
			allianceResults = [];
		} finally {
			loading = false;
		}
	}

	async function goToPage(page: number) {
		currentPage = page;
		await performSearch();
	}

	let hasNextPage = $derived(currentPage * perPage < totalResults);
	let hasPrevPage = $derived(currentPage > 1);
	let hasResults = $derived(
		activeSearchTab === 'pilots'
			? pilotResults.length > 0
			: activeSearchTab === 'corporations'
				? corpResults.length > 0
				: allianceResults.length > 0
	);
</script>

<div class="space-y-6">
	<div>
		<h1 class="text-2xl font-bold">Search</h1>
		<p class="mt-1 text-sm text-[var(--color-text-secondary)]">
			Search EVE Online pilots, corporations, and alliances
		</p>
	</div>

	<!-- Tabs -->
	<div class="flex gap-2 border-b border-[var(--color-border)] pb-2">
		{#each [
			{ id: 'pilots', label: 'Pilots' },
			{ id: 'corporations', label: 'Corporations' },
			{ id: 'alliances', label: 'Alliances' }
		] as tab}
			<button
				onclick={() => switchTab(tab.id as 'pilots' | 'corporations' | 'alliances')}
				class="rounded-t border-b-2 px-4 py-2 text-sm transition-colors"
				class:border-[var(--color-accent-blue)]={activeSearchTab === tab.id}
				class:text-[var(--color-accent-blue)]={activeSearchTab === tab.id}
				class:border-transparent={activeSearchTab !== tab.id}
				class:text-[var(--color-text-secondary)]={activeSearchTab !== tab.id}
			>
				{tab.label}
			</button>
		{/each}
	</div>

	<div class="relative">
		<input
			type="text"
			bind:value={query}
			oninput={onInput}
			placeholder="Search for {activeSearchTab}..."
			class="w-full rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] px-4 py-3 text-[var(--color-text-primary)] placeholder-[var(--color-text-secondary)] outline-none transition-colors focus:border-[var(--color-accent-blue)]"
		/>
		{#if loading}
			<div class="absolute right-4 top-1/2 -translate-y-1/2">
				<div
					class="h-5 w-5 animate-spin rounded-full border-2 border-[var(--color-border)] border-t-[var(--color-accent-blue)]"
				></div>
			</div>
		{/if}
	</div>

	{#if error}
		<div
			class="rounded-lg border border-[var(--color-accent-red)] bg-[var(--color-bg-secondary)] p-6 text-center"
		>
			<p class="text-[var(--color-accent-red)]">{error}</p>
		</div>
	{/if}

	{#if loading && !hasResults}
		<div class="space-y-2">
			{#each Array(5) as _}
				<div
					class="h-16 animate-pulse rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]"
				></div>
			{/each}
		</div>
	{:else if hasResults}
		<div
			class="divide-y divide-[var(--color-border)] rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]"
		>
			{#if activeSearchTab === 'pilots'}
				{#each pilotResults as character}
					<a
						href="/characters/{character.character_id}"
						class="flex items-center gap-4 px-5 py-4 no-underline transition-colors first:rounded-t-lg last:rounded-b-lg hover:bg-[var(--color-bg-tertiary)] hover:no-underline"
					>
						<img
							src="https://images.evetech.net/characters/{character.character_id}/portrait?size=64"
							alt={character.name}
							class="h-10 w-10 rounded-full"
						/>
						<div>
							<div class="font-medium text-[var(--color-text-primary)]">
								{character.name}
							</div>
							<div class="mt-0.5 text-xs text-[var(--color-text-secondary)]">
								ID: {character.character_id}
							</div>
						</div>
					</a>
				{/each}
			{:else if activeSearchTab === 'corporations'}
				{#each corpResults as corp}
					<a
						href="/corporations/{corp.corporation_id}"
						class="flex items-center gap-4 px-5 py-4 no-underline transition-colors first:rounded-t-lg last:rounded-b-lg hover:bg-[var(--color-bg-tertiary)] hover:no-underline"
					>
						<img
							src="https://images.evetech.net/corporations/{corp.corporation_id}/logo?size=64"
							alt={corp.name}
							class="h-10 w-10 rounded"
						/>
						<div>
							<div class="font-medium text-[var(--color-text-primary)]">
								{corp.name}
							</div>
							<div class="mt-0.5 text-xs text-[var(--color-text-secondary)]">
								{#if corp.member_count}
									{corp.member_count} members
								{:else}
									ID: {corp.corporation_id}
								{/if}
							</div>
						</div>
					</a>
				{/each}
			{:else}
				{#each allianceResults as alliance}
					<a
						href="/alliances/{alliance.alliance_id}"
						class="flex items-center gap-4 px-5 py-4 no-underline transition-colors first:rounded-t-lg last:rounded-b-lg hover:bg-[var(--color-bg-tertiary)] hover:no-underline"
					>
						<img
							src="https://images.evetech.net/alliances/{alliance.alliance_id}/logo?size=64"
							alt={alliance.name}
							class="h-10 w-10 rounded"
						/>
						<div>
							<div class="font-medium text-[var(--color-text-primary)]">
								{alliance.name}
								{#if alliance.ticker}
									<span class="text-[var(--color-text-secondary)]"
										>[{alliance.ticker}]</span
									>
								{/if}
							</div>
							<div class="mt-0.5 text-xs text-[var(--color-text-secondary)]">
								ID: {alliance.alliance_id}
							</div>
						</div>
					</a>
				{/each}
			{/if}
		</div>

		<div class="flex items-center justify-center gap-4">
			<button
				onclick={() => goToPage(currentPage - 1)}
				disabled={!hasPrevPage}
				class="rounded border border-[var(--color-border)] bg-[var(--color-bg-secondary)] px-4 py-2 text-sm text-[var(--color-text-secondary)] transition-colors hover:border-[var(--color-accent-blue)] hover:text-[var(--color-text-primary)] disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:border-[var(--color-border)] disabled:hover:text-[var(--color-text-secondary)]"
			>
				Previous
			</button>
			<span class="text-sm text-[var(--color-text-secondary)]"> Page {currentPage} </span>
			<button
				onclick={() => goToPage(currentPage + 1)}
				disabled={!hasNextPage}
				class="rounded border border-[var(--color-border)] bg-[var(--color-bg-secondary)] px-4 py-2 text-sm text-[var(--color-text-secondary)] transition-colors hover:border-[var(--color-accent-blue)] hover:text-[var(--color-text-primary)] disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:border-[var(--color-border)] disabled:hover:text-[var(--color-text-secondary)]"
			>
				Next
			</button>
		</div>
	{:else if searched && !loading}
		<div
			class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-12 text-center"
		>
			<p class="text-[var(--color-text-secondary)]">No {activeSearchTab} found for "{query}"</p>
		</div>
	{/if}
</div>
