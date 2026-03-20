<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { api } from '$lib/api/client';
	import type {
		CorporationDetail,
		DoctrineProfileData,
		ShipUsageEntry,
		DoctrineGroup,
		ShipTrend,
		FleetComp
	} from '$lib/api/client';
	import { formatNumber } from '$lib/utils/formatters';
	import FittingCard from '$lib/components/FittingCard.svelte';
	import KillLossTabs from '$lib/components/KillLossTabs.svelte';

	let corpId = $derived(Number($page.params.corpId));

	let detail = $state<CorporationDetail | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let selectedWindow = $state(30);

	onMount(() => {
		if (isNaN(corpId) || corpId <= 0) {
			error = `Invalid corporation ID: "${$page.params.corpId}"`;
			loading = false;
			return;
		}
		loadData();
	});

	async function loadData() {
		loading = true;
		error = null;
		try {
			detail = await api.getCorporation(corpId);
		} catch (e) {
			console.error('[nea] corporation detail load failed', e);
			error = e instanceof Error ? e.message : 'Failed to load corporation data';
		} finally {
			loading = false;
		}
	}

	let activeProfile = $derived<DoctrineProfileData | null>(
		detail?.profiles.find((p) => p.window_days === selectedWindow) ?? null
	);
	let shipUsage = $derived<ShipUsageEntry[]>(activeProfile?.ship_usage ?? []);
	let doctrines = $derived<DoctrineGroup[]>(activeProfile?.doctrines ?? []);
	let shipTrends = $derived<ShipTrend[]>(activeProfile?.ship_trends ?? []);
	let fleetComps = $derived<FleetComp[]>(activeProfile?.fleet_comps ?? []);
	let maxUsageCount = $derived(shipUsage.length > 0 ? shipUsage[0].count : 1);
</script>

<div class="space-y-8">
	{#if loading}
		<div class="space-y-6">
			<div class="flex items-center gap-6">
				<div class="h-20 w-20 animate-pulse rounded bg-[var(--color-bg-tertiary)]"></div>
				<div>
					<div class="h-8 w-48 animate-pulse rounded bg-[var(--color-bg-tertiary)]"></div>
					<div class="mt-2 h-4 w-32 animate-pulse rounded bg-[var(--color-bg-tertiary)]"></div>
				</div>
			</div>
		</div>
	{:else if error}
		<div
			class="rounded-lg border border-[var(--color-accent-red)] bg-[var(--color-bg-secondary)] p-6 text-center"
		>
			<p class="text-[var(--color-accent-red)]">{error}</p>
			<a href="/pilots" class="mt-4 inline-block text-sm">Back to Search</a>
		</div>
	{:else if detail}
		<!-- Header -->
		<section class="flex items-center gap-6">
			<img
				src="https://images.evetech.net/corporations/{corpId}/logo?size=128"
				alt={detail.corporation.name}
				class="h-20 w-20 rounded border-2 border-[var(--color-border)]"
			/>
			<div>
				<h1 class="text-2xl font-bold">{detail.corporation.name}</h1>
				<div class="mt-1 flex items-center gap-3 text-sm text-[var(--color-text-secondary)]">
					{#if detail.corporation.alliance_id}
						<a
							href="/alliances/{detail.corporation.alliance_id}"
							class="text-[var(--color-accent-blue)] hover:underline"
						>
							Alliance
						</a>
					{/if}
					{#if detail.corporation.member_count}
						<span>{formatNumber(detail.corporation.member_count)} members</span>
					{/if}
				</div>
			</div>
		</section>

		<!-- Window selector -->
		<div class="flex gap-2">
			{#each [7, 30, 90] as window}
				<button
					onclick={() => (selectedWindow = window)}
					class="rounded border px-4 py-2 text-sm transition-colors"
					class:border-[var(--color-accent-blue)]={selectedWindow === window}
					class:text-[var(--color-accent-blue)]={selectedWindow === window}
					class:bg-[var(--color-bg-tertiary)]={selectedWindow === window}
					class:border-[var(--color-border)]={selectedWindow !== window}
					class:text-[var(--color-text-secondary)]={selectedWindow !== window}
				>
					{window}d
				</button>
			{/each}
		</div>

		{#if activeProfile}
			<!-- Stats cards -->
			<section class="grid grid-cols-2 gap-4 lg:grid-cols-3">
				<div
					class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-4"
				>
					<div
						class="text-xs font-medium uppercase tracking-wide text-[var(--color-text-secondary)]"
					>
						Total Kills
					</div>
					<div class="mt-1 text-2xl font-bold text-[var(--color-accent-green)]">
						{formatNumber(activeProfile.total_kills)}
					</div>
				</div>
				<div
					class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-4"
				>
					<div
						class="text-xs font-medium uppercase tracking-wide text-[var(--color-text-secondary)]"
					>
						Total Losses
					</div>
					<div class="mt-1 text-2xl font-bold text-[var(--color-accent-red)]">
						{formatNumber(activeProfile.total_losses)}
					</div>
				</div>
				<div
					class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-4"
				>
					<div
						class="text-xs font-medium uppercase tracking-wide text-[var(--color-text-secondary)]"
					>
						Doctrines Detected
					</div>
					<div class="mt-1 text-2xl font-bold text-[var(--color-text-primary)]">
						{doctrines.length}
					</div>
				</div>
			</section>

			<!-- Ship Usage -->
			{#if shipUsage.length > 0}
				<section>
					<h2 class="mb-4 text-lg font-semibold">Ship Usage</h2>
					<div
						class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5"
					>
						<div class="space-y-2">
							{#each shipUsage as ship, i}
								<div class="flex items-center gap-3">
									<span
										class="w-5 text-right text-xs text-[var(--color-text-secondary)]"
										>{i + 1}</span
									>
									<img
										src="https://images.evetech.net/types/{ship.type_id}/icon?size=32"
										alt={ship.name}
										class="h-6 w-6"
									/>
									<div class="flex-1">
										<div class="flex items-center justify-between">
											<span class="text-sm text-[var(--color-text-primary)]"
												>{ship.name}</span
											>
											<span
												class="font-mono text-sm text-[var(--color-text-secondary)]"
												>{ship.count} ({ship.pct}%)</span
											>
										</div>
										<div
											class="mt-1 h-1.5 overflow-hidden rounded-full bg-[var(--color-bg-tertiary)]"
										>
											<div
												class="h-full rounded-full bg-[var(--color-accent-blue)]"
												style="width: {(ship.count / maxUsageCount) * 100}%"
											></div>
										</div>
									</div>
								</div>
							{/each}
						</div>
					</div>
				</section>
			{/if}

			<!-- Doctrines -->
			{#if doctrines.length > 0}
				<section>
					<h2 class="mb-4 text-lg font-semibold">Doctrines</h2>
					<div class="space-y-4">
						{#each doctrines as group, gi}
							<details class="group rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]" open>
								<summary class="flex cursor-pointer items-center justify-between p-4 select-none">
									<div class="flex flex-wrap items-center gap-2 overflow-hidden max-h-20">
										{#each group.ships as ship, si}
											<div class="flex items-center gap-1.5">
												<img
													src="https://images.evetech.net/types/{ship.ship_type_id}/icon?size=32"
													alt={ship.ship_name}
													class="h-6 w-6 shrink-0"
												/>
												<span class="whitespace-nowrap font-medium text-[var(--color-text-primary)]"
													>{ship.ship_name}</span
												>
											</div>
											{#if si < group.ships.length - 1}
												<span class="text-[var(--color-text-secondary)]">+</span>
											{/if}
										{/each}
									</div>
									<span
										class="ml-3 shrink-0 rounded-full bg-[color-mix(in_srgb,var(--color-accent-blue)_20%,transparent)] px-2 py-0.5 text-xs font-medium text-[var(--color-accent-blue)]"
									>
										{group.ships.length} ship{group.ships.length !== 1 ? 's' : ''}
									</span>
								</summary>

								<!-- Per-ship fits in a responsive grid -->
								<div class="grid grid-cols-1 gap-4 p-4 pt-0 lg:grid-cols-2 xl:grid-cols-3">
									{#each group.ships as ship, si}
										<div class="min-w-0">
											{#if ship.canonical_fit && ship.canonical_fit.length > 0}
												<FittingCard
													fitting={{
														ship_type_id: ship.ship_type_id,
														ship_name: ship.ship_name,
														modules: ship.canonical_fit,
														count: ship.occurrences,
														variant_count: ship.variants?.length ?? 0
													}}
												/>

												{#if ship.variants && ship.variants.length > 0}
													<details class="mt-2">
														<summary
															class="cursor-pointer text-xs text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]"
														>
															{ship.variants.length} variant fit{ship.variants.length !== 1 ? 's' : ''}
														</summary>
														<div class="mt-2 space-y-2 pl-2 border-l-2 border-[var(--color-border)]">
															{#each ship.variants as variant}
																<FittingCard
																	fitting={{
																		ship_type_id: ship.ship_type_id,
																		ship_name: ship.ship_name,
																		modules: variant,
																		count: 0,
																		variant_count: 0
																	}}
																/>
															{/each}
														</div>
													</details>
												{/if}
											{:else}
												<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-4">
													<div class="flex items-center gap-2 mb-2">
														<img
															src="https://images.evetech.net/types/{ship.ship_type_id}/icon?size=32"
															alt={ship.ship_name}
															class="h-6 w-6"
														/>
														<span class="font-medium text-[var(--color-text-primary)]">{ship.ship_name}</span>
													</div>
													<p class="text-xs text-[var(--color-text-secondary)] italic">
														No fit data available — no losses with this ship in the current window
													</p>
												</div>
											{/if}
										</div>
									{/each}
								</div>
							</details>
						{/each}
					</div>
				</section>
			{/if}

			<!-- Trends -->
			{#if shipTrends.length > 0}
				<section>
					<h2 class="mb-4 text-lg font-semibold">Ship Trends</h2>
					<div
						class="overflow-x-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]"
					>
						<table class="w-full text-left text-sm">
							<thead>
								<tr
									class="border-b border-[var(--color-border)] text-[var(--color-text-secondary)]"
								>
									<th class="px-4 py-3 font-medium">Ship</th>
									<th class="px-4 py-3 font-medium text-right">Current</th>
									<th class="px-4 py-3 font-medium text-right">Previous</th>
									<th class="px-4 py-3 font-medium text-right">Change</th>
								</tr>
							</thead>
							<tbody>
								{#each shipTrends as trend}
									<tr
										class="border-b border-[var(--color-border)] last:border-b-0 hover:bg-[var(--color-bg-tertiary)]"
									>
										<td class="px-4 py-3">
											<div class="flex items-center gap-2">
												<img
													src="https://images.evetech.net/types/{trend.type_id}/icon?size=32"
													alt={trend.name}
													class="h-5 w-5"
												/>
												<span class="text-[var(--color-text-primary)]"
													>{trend.name}</span
												>
											</div>
										</td>
										<td
											class="px-4 py-3 text-right font-mono text-[var(--color-text-secondary)]"
											>{trend.current_count}</td
										>
										<td
											class="px-4 py-3 text-right font-mono text-[var(--color-text-secondary)]"
											>{trend.previous_count}</td
										>
										<td class="px-4 py-3 text-right font-mono">
											<span
												class={trend.change_pct > 0
													? 'text-[var(--color-accent-green)]'
													: trend.change_pct < 0
														? 'text-[var(--color-accent-red)]'
														: 'text-[var(--color-text-secondary)]'}
											>
												{trend.change_pct > 0 ? '+' : ''}{trend.change_pct}%
											</span>
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				</section>
			{/if}

			<!-- Fleet Compositions -->
			{#if fleetComps.length > 0}
				<section>
					<h2 class="mb-4 text-lg font-semibold">Fleet Compositions</h2>
					<div
						class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5"
					>
						<div class="space-y-3">
							{#each fleetComps as comp}
								<div
									class="flex items-center justify-between rounded border border-[var(--color-border)] bg-[var(--color-bg-primary)] px-4 py-3"
								>
									<div class="flex items-center gap-4">
										{#each comp.ships as ship}
											<div class="flex items-center gap-1.5">
												<img
													src="https://images.evetech.net/types/{ship.type_id}/icon?size=32"
													alt={ship.name}
													class="h-5 w-5"
												/>
												<span class="text-sm text-[var(--color-text-primary)]"
													>{ship.name}</span
												>
											</div>
											{#if comp.ships.indexOf(ship) < comp.ships.length - 1}
												<span class="text-[var(--color-text-secondary)]">+</span>
											{/if}
										{/each}
									</div>
									<span class="font-mono text-sm text-[var(--color-text-secondary)]">
										{comp.occurrence_count} kills
									</span>
								</div>
							{/each}
						</div>
					</div>
				</section>
			{/if}
		{:else}
			<div
				class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-6 text-center text-sm text-[var(--color-text-secondary)]"
			>
				No doctrine data available for the {selectedWindow}-day window. Data will appear after
				the next aggregation cycle.
			</div>
		{/if}

		<KillLossTabs entityType="corporation" entityId={corpId} />
	{/if}
</div>
