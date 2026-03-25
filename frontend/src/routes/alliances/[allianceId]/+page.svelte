<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { api } from '$lib/api/client';
	import type { AllianceDetail } from '$lib/api/client';
	import { formatNumber } from '$lib/utils/formatters';
	import FittingCard from '$lib/components/FittingCard.svelte';
	import VariantOverlay from '$lib/components/VariantOverlay.svelte';
	import KillLossTabs from '$lib/components/KillLossTabs.svelte';
	import type { FittingModule } from '$lib/api/client';

	let variantOverlay = $state<{
		ship_type_id: number;
		ship_name: string;
		canonical_fit: FittingModule[];
		variants: FittingModule[][];
	} | null>(null);

	let allianceId = $derived(Number($page.params.allianceId));

	let detail = $state<AllianceDetail | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let selectedWindow = $state(30);

	onMount(() => {
		if (isNaN(allianceId) || allianceId <= 0) {
			error = `Invalid alliance ID: "${$page.params.allianceId}"`;
			loading = false;
			return;
		}
		loadData();
	});

	async function loadData() {
		loading = true;
		error = null;
		try {
			detail = await api.getAlliance(allianceId);
		} catch (e) {
			console.error('[nea] alliance detail load failed', e);
			error = e instanceof Error ? e.message : 'Failed to load alliance data';
		} finally {
			loading = false;
		}
	}

	let windows = [7, 30, 90] as const;
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
				src="https://images.evetech.net/alliances/{allianceId}/logo?size=128"
				alt={detail.alliance.name}
				class="h-20 w-20 rounded border-2 border-[var(--color-border)]"
			/>
			<div>
				<h1 class="text-2xl font-bold">
					{detail.alliance.name}
					{#if detail.alliance.ticker}
						<span class="text-lg text-[var(--color-text-secondary)]">[{detail.alliance.ticker}]</span>
					{/if}
				</h1>
			</div>
		</section>

		<!-- Window selector -->
		<div class="flex gap-2">
			{#each windows as window}
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

		{#each windows as window (window)}
			{@const profile = detail.profiles.find((p) => p.window_days === window) ?? null}
			{@const usage = profile?.ship_usage ?? []}
			{@const dcts = profile?.doctrines ?? []}
			{@const trends = profile?.ship_trends ?? []}
			{@const comps = profile?.fleet_comps ?? []}
			{@const maxCount = usage.length > 0 ? usage[0].count : 1}
			<div class="space-y-8" class:hidden={window !== selectedWindow}>
			{#if profile}
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
						{formatNumber(profile.total_kills)}
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
						{formatNumber(profile.total_losses)}
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
						{dcts.length}
					</div>
				</div>
			</section>

			<!-- Ship Usage -->
			{#if usage.length > 0}
				<section>
					<h2 class="mb-4 text-lg font-semibold">Ship Usage</h2>
					<div
						class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5"
					>
						<div class="space-y-2">
							{#each usage as ship, i}
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
												style="width: {(ship.count / maxCount) * 100}%"
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
			{#if dcts.length > 0}
				<section>
					<h2 class="mb-4 text-lg font-semibold">Doctrines</h2>
					<div class="space-y-4">
							{#each dcts as group, gi}
								<details class="group rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]" open>
									<summary class="flex cursor-pointer items-center justify-between p-4 select-none">
										<div class="min-w-0">
											<div class="flex max-h-20 flex-wrap items-center gap-2 overflow-hidden">
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
											<div class="mt-2 flex flex-wrap gap-2 text-xs text-[var(--color-text-secondary)]">
												{#if group.engagement_count}
													<span>{group.engagement_count} engagement{group.engagement_count !== 1 ? 's' : ''}</span>
												{/if}
												{#if group.distinct_pilot_count}
													<span>{group.distinct_pilot_count} pilot{group.distinct_pilot_count !== 1 ? 's' : ''}</span>
												{/if}
												{#if group.coverage_pct !== undefined}
													<span>{group.coverage_pct}% coverage</span>
												{/if}
												{#if group.mean_similarity !== undefined}
													<span>{Math.round(group.mean_similarity * 100)}% cohesion</span>
												{/if}
											</div>
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
													<button
														class="mt-2 rounded bg-[var(--color-bg-tertiary)] px-2.5 py-1 text-xs text-[var(--color-text-secondary)] hover:bg-[var(--color-border)] hover:text-[var(--color-text-primary)] transition-colors"
														onclick={() => (variantOverlay = {
															ship_type_id: ship.ship_type_id,
															ship_name: ship.ship_name,
															canonical_fit: ship.canonical_fit,
															variants: ship.variants
														})}
													>
														{ship.variants.length} variant{ship.variants.length !== 1 ? 's' : ''} — compare
													</button>
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
			{#if trends.length > 0}
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
								{#each trends as trend}
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
			{#if comps.length > 0}
				<section>
					<h2 class="mb-4 text-lg font-semibold">Fleet Compositions</h2>
					<div
						class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5"
					>
						<div class="space-y-3">
							{#each comps as comp}
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
				No doctrine data available for the {window}-day window. Data will appear after
				the next aggregation cycle.
			</div>
		{/if}
			</div>
		{/each}

		<KillLossTabs entityType="alliance" entityId={allianceId} />
	{/if}
</div>

{#if variantOverlay}
	<VariantOverlay
		ship_type_id={variantOverlay.ship_type_id}
		ship_name={variantOverlay.ship_name}
		canonical_fit={variantOverlay.canonical_fit}
		variants={variantOverlay.variants}
		onclose={() => (variantOverlay = null)}
	/>
{/if}
