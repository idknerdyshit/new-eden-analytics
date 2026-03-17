<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { api } from '$lib/api/client';
	import type {
		ItemDetail,
		MarketHistoryEntry,
		DestructionEntry,
		CorrelationResult
	} from '$lib/api/client';
	import DestructionChart from '$lib/charts/DestructionChart.svelte';
	import PriceImpactChart from '$lib/charts/PriceImpactChart.svelte';
	import CorrelationChart from '$lib/charts/CorrelationChart.svelte';
	import LagTimeline from '$lib/charts/LagTimeline.svelte';

	let typeId = $derived(Number($page.params.typeId));

	let item = $state<ItemDetail | null>(null);
	let marketHistory = $state<MarketHistoryEntry[]>([]);
	let destruction = $state<DestructionEntry[]>([]);
	let correlations = $state<CorrelationResult[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	onMount(() => {
		loadData();
	});

	async function loadData() {
		loading = true;
		error = null;
		try {
			const [itemData, historyData, destructionData, corrData] = await Promise.allSettled([
				api.getItem(typeId),
				api.marketHistory(typeId),
				api.destruction(typeId),
				api.correlations(typeId)
			]);

			if (itemData.status === 'fulfilled') item = itemData.value;
			else throw new Error('Failed to load item');

			if (historyData.status === 'fulfilled') {
				marketHistory = historyData.value;
			} else {
				console.warn('[nea] partial load failure: marketHistory', historyData.reason);
			}
			if (destructionData.status === 'fulfilled') {
				destruction = destructionData.value;
			} else {
				console.warn('[nea] partial load failure: destruction', destructionData.reason);
			}
			if (corrData.status === 'fulfilled') {
				correlations = corrData.value;
			} else {
				console.warn('[nea] partial load failure: correlations', corrData.reason);
			}
		} catch (e) {
			console.error('[nea] item detail load failed', e);
			error = e instanceof Error ? e.message : 'Failed to load item data';
		} finally {
			loading = false;
		}
	}

	function formatCorrelation(value: number): string {
		return value.toFixed(4);
	}

	function correlationColor(value: number): string {
		if (value > 0) return 'text-[var(--color-accent-blue)]';
		if (value < 0) return 'text-[var(--color-accent-red)]';
		return 'text-[var(--color-text-secondary)]';
	}

	function formatPValue(value: number | null): string {
		if (value === null) return '--';
		if (value < 0.001) return '< 0.001';
		return value.toFixed(4);
	}
</script>

<div class="space-y-8">
	{#if loading}
		<div class="space-y-6">
			<div class="h-8 w-64 animate-pulse rounded bg-[var(--color-bg-tertiary)]"></div>
			<div class="h-4 w-40 animate-pulse rounded bg-[var(--color-bg-tertiary)]"></div>
			<div class="grid grid-cols-1 gap-6 lg:grid-cols-2">
				<div class="h-64 animate-pulse rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]"></div>
				<div class="h-64 animate-pulse rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]"></div>
			</div>
		</div>
	{:else if error}
		<div class="rounded-lg border border-[var(--color-accent-red)] bg-[var(--color-bg-secondary)] p-6 text-center">
			<p class="text-[var(--color-accent-red)]">{error}</p>
			<a href="/" class="mt-4 inline-block text-sm">Back to Dashboard</a>
		</div>
	{:else if item}
		<!-- Item Info -->
		<section>
			<div class="flex items-start justify-between">
				<div>
					<h1 class="text-2xl font-bold">{item.item.name}</h1>
					<div class="mt-1 flex items-center gap-2 text-sm text-[var(--color-text-secondary)]">
						{#if item.item.category_name}
							<span>{item.item.category_name}</span>
						{/if}
						{#if item.item.category_name && item.item.group_name}
							<span>&rsaquo;</span>
						{/if}
						{#if item.item.group_name}
							<span>{item.item.group_name}</span>
						{/if}
						<span class="text-[var(--color-border)]">|</span>
						<span>ID: {item.item.type_id}</span>
					</div>
				</div>
			</div>

			<!-- Blueprint Materials -->
			{#if item.materials.length > 0}
				<div class="mt-6 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5">
					<h3 class="mb-3 text-sm font-semibold uppercase tracking-wide text-[var(--color-text-secondary)]">
						Blueprint Materials
					</h3>
					<div class="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">
						{#each item.materials as mat}
							<a
								href="/items/{mat.material_type_id}"
								class="flex items-center justify-between rounded border border-[var(--color-border)] px-3 py-2 no-underline transition-colors hover:border-[var(--color-accent-blue)] hover:no-underline"
							>
								<span class="text-sm text-[var(--color-text-primary)]">{mat.material_name}</span>
								<span class="text-sm font-mono text-[var(--color-text-secondary)]">
									x{mat.quantity.toLocaleString()}
								</span>
							</a>
						{/each}
					</div>
				</div>
			{/if}
		</section>

		<!-- Charts -->
		<section>
			<h2 class="mb-4 text-lg font-semibold">Charts</h2>
			<div class="grid grid-cols-1 gap-6 lg:grid-cols-2">
				<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5">
					<h3 class="mb-3 text-sm font-semibold uppercase tracking-wide text-[var(--color-text-secondary)]">
						Destruction Volume
					</h3>
					<DestructionChart
						data={destruction.map((d) => ({
							date: d.date,
							quantity_destroyed: d.quantity_destroyed,
							kill_count: d.kill_count
						}))}
					/>
				</div>

				<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5">
					<h3 class="mb-3 text-sm font-semibold uppercase tracking-wide text-[var(--color-text-secondary)]">
						Material Price Impact
					</h3>
					<PriceImpactChart
						priceData={marketHistory.map((h) => ({ date: h.date, average: h.average }))}
						destructionData={destruction.map((d) => ({
							date: d.date,
							quantity_destroyed: d.quantity_destroyed
						}))}
						materialName={item.item.name}
					/>
				</div>

				<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5 lg:col-span-2">
					<h3 class="mb-3 text-sm font-semibold uppercase tracking-wide text-[var(--color-text-secondary)]">
						Cross-Correlation Function (CCF)
					</h3>
					{#if correlations.length > 0}
						{@const bestCorr = correlations.reduce((a, b) =>
							Math.abs(a.correlation_coeff) > Math.abs(b.correlation_coeff) ? a : b
						)}
						<CorrelationChart
							data={correlations.map((c) => ({ lag: c.lag_days, correlation: c.correlation_coeff }))}
							optimalLag={bestCorr.lag_days}
							confidenceThreshold={1.96 / Math.sqrt(correlations.length)}
						/>
					{:else}
						<div class="flex h-64 items-center justify-center text-sm text-[var(--color-text-secondary)]">
							No correlation data
						</div>
					{/if}
				</div>
			</div>
		</section>

		<!-- Lag Timeline -->
		{#if correlations.length > 0}
			<section>
				<h2 class="mb-4 text-lg font-semibold">Lag Timeline</h2>
				<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5">
					<LagTimeline
						correlations={correlations.map((c) => ({
							material_name: `Type ${c.material_type_id}`,
							lag_days: c.lag_days,
							correlation_coeff: c.correlation_coeff,
							granger_significant: c.granger_significant
						}))}
					/>
				</div>
			</section>
		{/if}

		<!-- Correlation Analysis Table -->
		<section>
			<h2 class="mb-4 text-lg font-semibold">Correlation Analysis</h2>
			{#if correlations.length > 0}
				<div class="overflow-x-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]">
					<table class="w-full text-left text-sm">
						<thead>
							<tr class="border-b border-[var(--color-border)] text-[var(--color-text-secondary)]">
								<th class="px-4 py-3 font-medium text-right">Lag (days)</th>
								<th class="px-4 py-3 font-medium text-right">Correlation</th>
								<th class="px-4 py-3 font-medium text-right">Granger F-stat</th>
								<th class="px-4 py-3 font-medium text-right">Granger p-value</th>
								<th class="px-4 py-3 font-medium text-center">Significant</th>
								<th class="px-4 py-3 font-medium">Window</th>
							</tr>
						</thead>
						<tbody>
							{#each correlations as corr}
								<tr class="border-b border-[var(--color-border)] last:border-b-0 hover:bg-[var(--color-bg-tertiary)]">
									<td class="px-4 py-3 text-right text-[var(--color-text-secondary)]">
										{corr.lag_days}
									</td>
									<td class="px-4 py-3 text-right font-mono {correlationColor(corr.correlation_coeff)}">
										{formatCorrelation(corr.correlation_coeff)}
									</td>
									<td class="px-4 py-3 text-right font-mono text-[var(--color-text-secondary)]">
										{corr.granger_f_stat !== null ? corr.granger_f_stat.toFixed(2) : '--'}
									</td>
									<td class="px-4 py-3 text-right font-mono text-[var(--color-text-secondary)]">
										{formatPValue(corr.granger_p_value)}
									</td>
									<td class="px-4 py-3 text-center">
										{#if corr.granger_significant}
											<span class="text-[var(--color-accent-green)]" title="Statistically significant">&#10003;</span>
										{:else}
											<span class="text-[var(--color-text-secondary)]">&#8212;</span>
										{/if}
									</td>
									<td class="px-4 py-3 text-xs text-[var(--color-text-secondary)]">
										{corr.window_start.slice(0, 10)} &mdash; {corr.window_end.slice(0, 10)}
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{:else}
				<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-8 text-center text-[var(--color-text-secondary)]">
					No correlation analysis data available for this item.
				</div>
			{/if}
		</section>
	{/if}
</div>
