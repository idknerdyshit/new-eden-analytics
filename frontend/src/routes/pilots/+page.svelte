<script lang="ts">
	import { api } from '$lib/api/client';
	import type { CharacterInfo } from '$lib/api/client';

	let query = $state('');
	let results = $state<CharacterInfo[]>([]);
	let currentPage = $state(1);
	let perPage = $state(20);
	let totalResults = $state(0);
	let loading = $state(false);
	let searched = $state(false);
	let error = $state<string | null>(null);

	let debounceTimer: ReturnType<typeof setTimeout> | undefined;

	function onInput() {
		currentPage = 1;
		clearTimeout(debounceTimer);
		debounceTimer = setTimeout(() => {
			performSearch();
		}, 300);
	}

	async function performSearch() {
		const q = query.trim();
		if (!q) {
			results = [];
			searched = false;
			return;
		}

		loading = true;
		searched = true;
		error = null;
		try {
			const data = await api.searchCharacters(q, currentPage);
			results = data.characters;
			perPage = data.per_page;
			totalResults = data.total;
		} catch (e) {
			console.error('[nea] character search failed', e);
			error = e instanceof Error ? e.message : 'Search failed';
			results = [];
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
</script>

<div class="space-y-6">
	<div>
		<h1 class="text-2xl font-bold">Search Pilots</h1>
		<p class="mt-1 text-sm text-[var(--color-text-secondary)]">
			Search EVE Online characters by name
		</p>
	</div>

	<div class="relative">
		<input
			type="text"
			bind:value={query}
			oninput={onInput}
			placeholder="Search for pilots..."
			class="w-full rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] px-4 py-3 text-[var(--color-text-primary)] placeholder-[var(--color-text-secondary)] outline-none transition-colors focus:border-[var(--color-accent-blue)]"
		/>
		{#if loading}
			<div class="absolute right-4 top-1/2 -translate-y-1/2">
				<div class="h-5 w-5 animate-spin rounded-full border-2 border-[var(--color-border)] border-t-[var(--color-accent-blue)]"></div>
			</div>
		{/if}
	</div>

	{#if error}
		<div class="rounded-lg border border-[var(--color-accent-red)] bg-[var(--color-bg-secondary)] p-6 text-center">
			<p class="text-[var(--color-accent-red)]">{error}</p>
		</div>
	{/if}

	{#if loading && results.length === 0}
		<div class="space-y-2">
			{#each Array(5) as _}
				<div class="h-16 animate-pulse rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]"></div>
			{/each}
		</div>
	{:else if results.length > 0}
		<div class="divide-y divide-[var(--color-border)] rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]">
			{#each results as character}
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
						<div class="font-medium text-[var(--color-text-primary)]">{character.name}</div>
						<div class="mt-0.5 text-xs text-[var(--color-text-secondary)]">
							ID: {character.character_id}
						</div>
					</div>
				</a>
			{/each}
		</div>

		<div class="flex items-center justify-center gap-4">
			<button
				onclick={() => goToPage(currentPage - 1)}
				disabled={!hasPrevPage}
				class="rounded border border-[var(--color-border)] bg-[var(--color-bg-secondary)] px-4 py-2 text-sm text-[var(--color-text-secondary)] transition-colors hover:border-[var(--color-accent-blue)] hover:text-[var(--color-text-primary)] disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:border-[var(--color-border)] disabled:hover:text-[var(--color-text-secondary)]"
			>
				Previous
			</button>
			<span class="text-sm text-[var(--color-text-secondary)]">
				Page {currentPage}
			</span>
			<button
				onclick={() => goToPage(currentPage + 1)}
				disabled={!hasNextPage}
				class="rounded border border-[var(--color-border)] bg-[var(--color-bg-secondary)] px-4 py-2 text-sm text-[var(--color-text-secondary)] transition-colors hover:border-[var(--color-accent-blue)] hover:text-[var(--color-text-primary)] disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:border-[var(--color-border)] disabled:hover:text-[var(--color-text-secondary)]"
			>
				Next
			</button>
		</div>
	{:else if searched && !loading}
		<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-12 text-center">
			<p class="text-[var(--color-text-secondary)]">No pilots found for "{query}"</p>
		</div>
	{/if}
</div>
