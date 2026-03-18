<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';
	import type { DashboardData, Mover, CorrelationResult, DestructionEntry } from '$lib/api/client';
	import { correlationColor, formatCorrelation, formatNumber, formatPrice, changeColor, changeArrow } from '$lib/utils/formatters';

	let dashboard = $state<DashboardData | null>(null);
	let movers = $state<Mover[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	onMount(async () => {
		try {
			const [dashData, moverData] = await Promise.all([
				api.dashboard(),
				api.movers()
			]);
			dashboard = dashData;
			movers = moverData;
		} catch (e) {
			console.error('[nea] dashboard load failed', e);
			error = e instanceof Error ? e.message : 'Failed to load dashboard';
		} finally {
			loading = false;
		}
	});
</script>

<div class="space-y-10">
	<div>
		<h1 class="text-2xl font-bold">Dashboard</h1>
		<p class="mt-1 text-sm text-[var(--color-text-secondary)]">
			EVE Online market analytics — correlating destruction with material prices
		</p>
	</div>

	{#if loading}
		<div class="space-y-6">
			<!-- Skeleton for correlations table -->
			<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-6">
				<div class="mb-4 h-6 w-48 animate-pulse rounded bg-[var(--color-bg-tertiary)]"></div>
				{#each Array(5) as _}
					<div class="mb-3 h-10 animate-pulse rounded bg-[var(--color-bg-tertiary)]"></div>
				{/each}
			</div>
			<!-- Skeleton for cards -->
			<div class="grid grid-cols-1 gap-4 md:grid-cols-3">
				{#each Array(3) as _}
					<div class="h-32 animate-pulse rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]"></div>
				{/each}
			</div>
		</div>
	{:else if error}
		<div class="rounded-lg border border-[var(--color-accent-red)] bg-[var(--color-bg-secondary)] p-6 text-center">
			<p class="text-[var(--color-accent-red)]">{error}</p>
		</div>
	{:else}
		<!-- Top Correlations -->
		<section>
			<h2 class="mb-4 text-lg font-semibold">Top Correlations</h2>
			{#if dashboard && dashboard.top_correlations.length > 0}
				<div class="overflow-x-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]">
					<table class="w-full text-left text-sm">
						<thead>
							<tr class="border-b border-[var(--color-border)] text-[var(--color-text-secondary)]">
								<th class="px-4 py-3 font-medium">Product</th>
								<th class="px-4 py-3 font-medium">Material</th>
								<th class="px-4 py-3 font-medium text-right">Lag (days)</th>
								<th class="px-4 py-3 font-medium text-right">Correlation</th>
								<th class="px-4 py-3 font-medium text-center">Significant</th>
							</tr>
						</thead>
						<tbody>
							{#each dashboard.top_correlations as corr}
								<tr class="border-b border-[var(--color-border)] transition-colors last:border-b-0 hover:bg-[var(--color-bg-tertiary)]">
									<td class="px-4 py-3">
										<a href="/items/{corr.product_type_id}" class="no-underline hover:underline">
											{corr.product_name}
										</a>
									</td>
									<td class="px-4 py-3">
										<a href="/items/{corr.material_type_id}" class="no-underline hover:underline">
											{corr.material_name}
										</a>
									</td>
									<td class="px-4 py-3 text-right text-[var(--color-text-secondary)]">
										{corr.lag_days}
									</td>
									<td class="px-4 py-3 text-right font-mono {correlationColor(corr.correlation_coeff)}">
										{formatCorrelation(corr.correlation_coeff)}
									</td>
									<td class="px-4 py-3 text-center">
										{#if corr.granger_significant}
											<span class="text-[var(--color-accent-green)]" title="Granger-significant">&#10003;</span>
										{:else}
											<span class="text-[var(--color-text-secondary)]">&#8212;</span>
										{/if}
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{:else}
				<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-8 text-center text-[var(--color-text-secondary)]">
					No correlation data available yet.
				</div>
			{/if}
		</section>

		<!-- Trending Destruction -->
		<section>
			<h2 class="mb-4 text-lg font-semibold">Trending Destruction</h2>
			{#if dashboard && dashboard.top_destruction.length > 0}
				<div class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
					{#each dashboard.top_destruction as entry}
						<a
							href="/items/{entry.type_id}"
							class="group rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5 no-underline transition-colors hover:border-[var(--color-accent-blue)] hover:no-underline"
						>
							<div class="mb-2 text-sm font-medium text-[var(--color-text-primary)] group-hover:text-[var(--color-accent-blue)]">
								{entry.type_name ?? `Type ${entry.type_id}`}
							</div>
							<div class="flex items-end justify-between">
								<div>
									<div class="text-xs text-[var(--color-text-secondary)]">Units Destroyed</div>
									<div class="text-lg font-bold text-[var(--color-accent-red)]">
										{formatNumber(entry.quantity_destroyed)}
									</div>
								</div>
								<div class="text-right">
									<div class="text-xs text-[var(--color-text-secondary)]">Killmails</div>
									<div class="text-lg font-semibold text-[var(--color-text-primary)]">
										{formatNumber(entry.kill_count)}
									</div>
								</div>
							</div>
							<div class="mt-2 text-xs text-[var(--color-text-secondary)]">
								{entry.date}
							</div>
						</a>
					{/each}
				</div>
			{:else}
				<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-8 text-center text-[var(--color-text-secondary)]">
					No destruction data available yet.
				</div>
			{/if}
		</section>

		<!-- Biggest Material Movers -->
		<section>
			<h2 class="mb-4 text-lg font-semibold">Biggest Material Movers</h2>
			{#if movers.length > 0}
				<div class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
					{#each movers as mover}
						<a
							href="/items/{mover.type_id}"
							class="group rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5 no-underline transition-colors hover:border-[var(--color-accent-blue)] hover:no-underline"
						>
							<div class="mb-2 text-sm font-medium text-[var(--color-text-primary)] group-hover:text-[var(--color-accent-blue)]">
								{mover.name}
							</div>
							<div class="flex items-end justify-between">
								<div>
									<div class="text-xs text-[var(--color-text-secondary)]">Price</div>
									<div class="text-lg font-bold text-[var(--color-text-primary)]">
										{formatPrice(mover.current_avg)} ISK
									</div>
								</div>
								<div class="text-right">
									<div class="text-xs text-[var(--color-text-secondary)]">24h Change</div>
									<div class="text-lg font-semibold {changeColor(mover.change_pct)}">
										{changeArrow(mover.change_pct)}{mover.change_pct.toFixed(2)}%
									</div>
								</div>
							</div>
						</a>
					{/each}
				</div>
			{:else}
				<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-8 text-center text-[var(--color-text-secondary)]">
					No price movement data available yet.
				</div>
			{/if}
		</section>
	{/if}
</div>
