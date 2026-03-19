<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { api } from '$lib/api/client';
	import type {
		CharacterDetail,
		KillmailEntry,
		ShipCount,
		FittingCluster
	} from '$lib/api/client';
	import { formatNumber, formatPrice } from '$lib/utils/formatters';
	import FittingCard from '$lib/components/FittingCard.svelte';

	let characterId = $derived(Number($page.params.characterId));

	let detail = $state<CharacterDetail | null>(null);
	let kills = $state<KillmailEntry[]>([]);
	let losses = $state<KillmailEntry[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let warnings = $state<string[]>([]);
	let activeTab = $state<'kills' | 'losses'>('kills');

	onMount(() => {
		if (isNaN(characterId) || characterId <= 0) {
			error = `Invalid character ID: "${$page.params.characterId}"`;
			loading = false;
			return;
		}
		loadData();
	});

	async function loadData() {
		loading = true;
		error = null;
		try {
			const [charData, killsData, lossesData] = await Promise.allSettled([
				api.getCharacter(characterId),
				api.getCharacterKills(characterId),
				api.getCharacterLosses(characterId)
			]);

			if (charData.status === 'fulfilled') detail = charData.value;
			else throw new Error('Failed to load character');

			const loadWarnings: string[] = [];
			if (killsData.status === 'fulfilled') {
				kills = killsData.value;
			} else {
				console.warn('[nea] partial load failure: kills', killsData.reason);
				loadWarnings.push('Kill data unavailable');
			}
			if (lossesData.status === 'fulfilled') {
				losses = lossesData.value;
			} else {
				console.warn('[nea] partial load failure: losses', lossesData.reason);
				loadWarnings.push('Loss data unavailable');
			}
			warnings = loadWarnings;
		} catch (e) {
			console.error('[nea] character detail load failed', e);
			error = e instanceof Error ? e.message : 'Failed to load character data';
		} finally {
			loading = false;
		}
	}

	let profile = $derived(detail?.profile ?? null);
	let soloKillPct = $derived(
		profile && profile.total_kills > 0
			? Math.round((profile.solo_kills / profile.total_kills) * 100)
			: 0
	);
	let soloLossPct = $derived(
		profile && profile.total_losses > 0
			? Math.round((profile.solo_losses / profile.total_losses) * 100)
			: 0
	);
	let playStyle = $derived(
		soloKillPct > 70 ? 'Mostly Solo' : soloKillPct > 30 ? 'Small Gang' : 'Fleet Pilot'
	);
	let playStyleColor = $derived(
		soloKillPct > 70
			? 'var(--color-accent-green)'
			: soloKillPct > 30
				? 'var(--color-accent-blue)'
				: '#a371f7'
	);

	let shipsFlown = $derived<ShipCount[]>(profile?.top_ships_flown ?? []);
	let shipsLost = $derived<ShipCount[]>(profile?.top_ships_lost ?? []);
	let commonFits = $derived<FittingCluster[]>(profile?.common_fits ?? []);
</script>

<div class="space-y-8">
	{#if loading}
		<div class="space-y-6">
			<div class="flex items-center gap-6">
				<div class="h-20 w-20 animate-pulse rounded-full bg-[var(--color-bg-tertiary)]"></div>
				<div>
					<div class="h-8 w-48 animate-pulse rounded bg-[var(--color-bg-tertiary)]"></div>
					<div class="mt-2 h-4 w-32 animate-pulse rounded bg-[var(--color-bg-tertiary)]"></div>
				</div>
			</div>
			<div class="grid grid-cols-2 gap-4 lg:grid-cols-4">
				{#each Array(4) as _}
					<div class="h-24 animate-pulse rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]"></div>
				{/each}
			</div>
		</div>
	{:else if error}
		<div class="rounded-lg border border-[var(--color-accent-red)] bg-[var(--color-bg-secondary)] p-6 text-center">
			<p class="text-[var(--color-accent-red)]">{error}</p>
			<a href="/pilots" class="mt-4 inline-block text-sm">Back to Pilots</a>
		</div>
	{:else if detail}
		<!-- Header -->
		<section class="flex items-center gap-6">
			<img
				src="https://images.evetech.net/characters/{characterId}/portrait?size=128"
				alt={detail.character.name}
				class="h-20 w-20 rounded-full border-2 border-[var(--color-border)]"
			/>
			<div>
				<h1 class="text-2xl font-bold">{detail.character.name}</h1>
				<div class="mt-1 flex items-center gap-3 text-sm text-[var(--color-text-secondary)]">
					<span>ID: {characterId}</span>
					{#if detail.character.corporation_id}
						<a
							href="/corporations/{detail.character.corporation_id}"
							class="text-[var(--color-accent-blue)] hover:underline"
						>
							Corporation
						</a>
					{/if}
					{#if detail.character.alliance_id}
						<a
							href="/alliances/{detail.character.alliance_id}"
							class="text-[var(--color-accent-blue)] hover:underline"
						>
							Alliance
						</a>
					{/if}
				</div>
				{#if profile}
					<div class="mt-2">
						<span
							class="rounded-full px-3 py-1 text-xs font-medium"
							style="background: color-mix(in srgb, {playStyleColor} 20%, transparent); color: {playStyleColor}; border: 1px solid color-mix(in srgb, {playStyleColor} 40%, transparent);"
						>
							{playStyle}
						</span>
					</div>
				{/if}
			</div>
		</section>

		{#if warnings.length > 0}
			<div class="rounded-lg border border-[var(--color-accent-yellow,#d29922)] bg-[var(--color-bg-secondary)] p-4">
				{#each warnings as warning}
					<p class="text-sm text-[var(--color-accent-yellow,#d29922)]">{warning}</p>
				{/each}
			</div>
		{/if}

		<!-- Stats Cards -->
		{#if profile}
			<section class="grid grid-cols-2 gap-4 lg:grid-cols-4">
				<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-4">
					<div class="text-xs font-medium uppercase tracking-wide text-[var(--color-text-secondary)]">
						Total Kills
					</div>
					<div class="mt-1 text-2xl font-bold text-[var(--color-accent-green)]">
						{formatNumber(profile.total_kills)}
					</div>
				</div>
				<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-4">
					<div class="text-xs font-medium uppercase tracking-wide text-[var(--color-text-secondary)]">
						Total Losses
					</div>
					<div class="mt-1 text-2xl font-bold text-[var(--color-accent-red)]">
						{formatNumber(profile.total_losses)}
					</div>
				</div>
				<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-4">
					<div class="text-xs font-medium uppercase tracking-wide text-[var(--color-text-secondary)]">
						Solo Kill %
					</div>
					<div class="mt-1 text-2xl font-bold text-[var(--color-text-primary)]">
						{soloKillPct}%
					</div>
				</div>
				<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-4">
					<div class="text-xs font-medium uppercase tracking-wide text-[var(--color-text-secondary)]">
						Solo Loss %
					</div>
					<div class="mt-1 text-2xl font-bold text-[var(--color-text-primary)]">
						{soloLossPct}%
					</div>
				</div>
			</section>
		{:else}
			<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-6 text-center text-sm text-[var(--color-text-secondary)]">
				Profile not yet computed. Data will appear after the next aggregation cycle.
			</div>
		{/if}

		<!-- Ships Flown / Lost -->
		{#if shipsFlown.length > 0 || shipsLost.length > 0}
			<section class="grid grid-cols-1 gap-6 lg:grid-cols-2">
				{#if shipsFlown.length > 0}
					<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5">
						<h3 class="mb-3 text-sm font-semibold uppercase tracking-wide text-[var(--color-text-secondary)]">
							Ships Flown
						</h3>
						<div class="space-y-2">
							{#each shipsFlown as ship, i}
								<div class="flex items-center justify-between">
									<div class="flex items-center gap-3">
										<span class="w-5 text-right text-xs text-[var(--color-text-secondary)]">{i + 1}</span>
										<img
											src="https://images.evetech.net/types/{ship.type_id}/icon?size=32"
											alt={ship.name}
											class="h-6 w-6"
										/>
										<span class="text-sm text-[var(--color-text-primary)]">{ship.name}</span>
									</div>
									<span class="font-mono text-sm text-[var(--color-text-secondary)]">{ship.count}</span>
								</div>
							{/each}
						</div>
					</div>
				{/if}

				{#if shipsLost.length > 0}
					<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5">
						<h3 class="mb-3 text-sm font-semibold uppercase tracking-wide text-[var(--color-text-secondary)]">
							Ships Lost
						</h3>
						<div class="space-y-2">
							{#each shipsLost as ship, i}
								<div class="flex items-center justify-between">
									<div class="flex items-center gap-3">
										<span class="w-5 text-right text-xs text-[var(--color-text-secondary)]">{i + 1}</span>
										<img
											src="https://images.evetech.net/types/{ship.type_id}/icon?size=32"
											alt={ship.name}
											class="h-6 w-6"
										/>
										<span class="text-sm text-[var(--color-text-primary)]">{ship.name}</span>
									</div>
									<span class="font-mono text-sm text-[var(--color-text-secondary)]">{ship.count}</span>
								</div>
							{/each}
						</div>
					</div>
				{/if}
			</section>
		{/if}

		<!-- Common Fittings -->
		{#if commonFits.length > 0}
			<section>
				<h2 class="mb-4 text-lg font-semibold">Common Fittings</h2>
				<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
					{#each commonFits as fit}
						<FittingCard fitting={fit} />
					{/each}
				</div>
			</section>
		{/if}

		<!-- Recent Activity -->
		<section>
			<h2 class="mb-4 text-lg font-semibold">Recent Activity</h2>
			<div class="mb-4 flex gap-2">
				<button
					onclick={() => (activeTab = 'kills')}
					class="rounded border px-4 py-2 text-sm transition-colors"
					class:border-[var(--color-accent-blue)]={activeTab === 'kills'}
					class:text-[var(--color-accent-blue)]={activeTab === 'kills'}
					class:border-[var(--color-border)]={activeTab !== 'kills'}
					class:text-[var(--color-text-secondary)]={activeTab !== 'kills'}
				>
					Kills ({kills.length})
				</button>
				<button
					onclick={() => (activeTab = 'losses')}
					class="rounded border px-4 py-2 text-sm transition-colors"
					class:border-[var(--color-accent-red)]={activeTab === 'losses'}
					class:text-[var(--color-accent-red)]={activeTab === 'losses'}
					class:border-[var(--color-border)]={activeTab !== 'losses'}
					class:text-[var(--color-text-secondary)]={activeTab !== 'losses'}
				>
					Losses ({losses.length})
				</button>
			</div>

			{#if (activeTab === 'kills' ? kills : losses).length > 0}
				{@const activeData = activeTab === 'kills' ? kills : losses}
				<div class="overflow-x-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]">
					<table class="w-full text-left text-sm">
						<thead>
							<tr class="border-b border-[var(--color-border)] text-[var(--color-text-secondary)]">
								<th class="px-4 py-3 font-medium">Killmail ID</th>
								<th class="px-4 py-3 font-medium">Time</th>
								<th class="px-4 py-3 font-medium text-right">Value</th>
							</tr>
						</thead>
						<tbody>
							{#each activeData as km}
								<tr class="border-b border-[var(--color-border)] last:border-b-0 hover:bg-[var(--color-bg-tertiary)]">
									<td class="px-4 py-3 font-mono text-[var(--color-text-primary)]">
										{km.killmail_id}
									</td>
									<td class="px-4 py-3 text-[var(--color-text-secondary)]">
										{new Date(km.kill_time).toLocaleString()}
									</td>
									<td class="px-4 py-3 text-right font-mono text-[var(--color-text-secondary)]">
										{km.total_value != null ? formatPrice(km.total_value) : '--'}
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{:else}
				<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-8 text-center text-[var(--color-text-secondary)]">
					No {activeTab} data available.
				</div>
			{/if}
		</section>
	{/if}
</div>
