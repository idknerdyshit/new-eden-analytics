<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { api } from '$lib/api/client';
	import type { KillmailDetailData, KillmailItemDetail } from '$lib/api/client';
	import { formatPrice, formatNumber } from '$lib/utils/formatters';

	let killmailId = $derived(Number($page.params.killmailId));

	let detail = $state<KillmailDetailData | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);

	onMount(() => {
		if (isNaN(killmailId) || killmailId <= 0) {
			error = `Invalid killmail ID: "${$page.params.killmailId}"`;
			loading = false;
			return;
		}
		loadData();
	});

	async function loadData() {
		loading = true;
		error = null;
		try {
			detail = await api.getKillmail(killmailId);
		} catch (e) {
			console.error('[nea] killmail detail load failed', e);
			error = e instanceof Error ? e.message : 'Failed to load killmail';
		} finally {
			loading = false;
		}
	}

	let destroyedItems = $derived<KillmailItemDetail[]>(
		detail?.items.filter((i) => i.quantity_destroyed > 0) ?? []
	);
	let droppedItems = $derived<KillmailItemDetail[]>(
		detail?.items.filter((i) => i.quantity_dropped > 0) ?? []
	);
	let totalDamage = $derived(
		detail?.attackers.reduce((sum, a) => sum + a.damage_done, 0) ?? 0
	);
</script>

<div class="space-y-8">
	{#if loading}
		<div class="space-y-6">
			<div class="h-8 w-64 animate-pulse rounded bg-[var(--color-bg-tertiary)]"></div>
			<div class="h-40 animate-pulse rounded-lg bg-[var(--color-bg-tertiary)]"></div>
			<div class="h-60 animate-pulse rounded-lg bg-[var(--color-bg-tertiary)]"></div>
		</div>
	{:else if error}
		<div class="rounded-lg border border-[var(--color-accent-red)] bg-[var(--color-bg-secondary)] p-6 text-center">
			<p class="text-[var(--color-accent-red)]">{error}</p>
		</div>
	{:else if detail}
		<!-- Header -->
		<section>
			<h1 class="text-2xl font-bold">Killmail #{killmailId}</h1>
			<div class="mt-1 flex items-center gap-4 text-sm text-[var(--color-text-secondary)]">
				<span>{new Date(detail.killmail.kill_time).toLocaleString()}</span>
				{#if detail.killmail.total_value != null}
					<span class="font-mono">{formatPrice(detail.killmail.total_value)} ISK</span>
				{/if}
			</div>
		</section>

		<!-- Victim Card -->
		<section class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-5">
			<h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-[var(--color-text-secondary)]">Victim</h2>
			<div class="flex items-center gap-4">
				{#if detail.victim.ship_type_id}
					<img
						src="https://images.evetech.net/types/{detail.victim.ship_type_id}/icon?size=64"
						alt={detail.victim.ship_name ?? 'Unknown'}
						class="h-16 w-16 rounded border border-[var(--color-border)]"
					/>
				{/if}
				<div>
					<div class="text-lg font-medium text-[var(--color-text-primary)]">
						{detail.victim.ship_name ?? 'Unknown Ship'}
					</div>
					<div class="mt-1 flex flex-wrap items-center gap-3 text-sm">
						{#if detail.victim.character_name}
							<a
								href="/characters/{detail.victim.character_id}"
								class="text-[var(--color-accent-blue)] hover:underline"
							>
								{detail.victim.character_name}
							</a>
						{/if}
						{#if detail.victim.corporation_name}
							<a
								href="/corporations/{detail.victim.corporation_id}"
								class="text-[var(--color-text-secondary)] hover:underline"
							>
								{detail.victim.corporation_name}
							</a>
						{/if}
						{#if detail.victim.alliance_name}
							<a
								href="/alliances/{detail.victim.alliance_id}"
								class="text-[var(--color-text-secondary)] hover:underline"
							>
								[{detail.victim.alliance_name}]
							</a>
						{/if}
					</div>
				</div>
			</div>
		</section>

		<!-- Attackers -->
		{#if detail.attackers.length > 0}
			<section>
				<h2 class="mb-4 text-lg font-semibold">Attackers ({detail.attackers.length})</h2>
				<div class="overflow-x-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]">
					<table class="w-full text-left text-sm">
						<thead>
							<tr class="border-b border-[var(--color-border)] text-[var(--color-text-secondary)]">
								<th class="px-4 py-3 font-medium">Ship</th>
								<th class="px-4 py-3 font-medium">Pilot</th>
								<th class="px-4 py-3 font-medium">Corporation</th>
								<th class="px-4 py-3 font-medium">Weapon</th>
								<th class="px-4 py-3 font-medium text-right">Damage</th>
							</tr>
						</thead>
						<tbody>
							{#each detail.attackers as attacker}
								<tr
									class="border-b border-[var(--color-border)] last:border-b-0"
									class:bg-[color-mix(in_srgb,var(--color-accent-green)_8%,transparent)]={attacker.final_blow}
								>
									<td class="px-4 py-3">
										<div class="flex items-center gap-2">
											<img
												src="https://images.evetech.net/types/{attacker.ship_type_id}/icon?size=32"
												alt={attacker.ship_name ?? 'Unknown'}
												class="h-6 w-6"
											/>
											<span class="text-[var(--color-text-primary)]">
												{attacker.ship_name ?? 'Unknown'}
											</span>
										</div>
									</td>
									<td class="px-4 py-3">
										{#if attacker.character_name}
											<a
												href="/characters/{attacker.character_id}"
												class="text-[var(--color-accent-blue)] hover:underline"
											>
												{attacker.character_name}
											</a>
										{:else}
											<span class="text-[var(--color-text-secondary)]">--</span>
										{/if}
									</td>
									<td class="px-4 py-3 text-[var(--color-text-secondary)]">
										{#if attacker.corporation_name}
											<a
												href="/corporations/{attacker.corporation_id}"
												class="hover:underline"
											>
												{attacker.corporation_name}
											</a>
										{:else}
											--
										{/if}
									</td>
									<td class="px-4 py-3 text-[var(--color-text-secondary)]">
										{attacker.weapon_name ?? '--'}
									</td>
									<td class="px-4 py-3 text-right font-mono">
										<span class={attacker.final_blow ? 'font-bold text-[var(--color-accent-green)]' : 'text-[var(--color-text-secondary)]'}>
											{formatNumber(attacker.damage_done)}
										</span>
										{#if totalDamage > 0}
											<span class="ml-1 text-xs text-[var(--color-text-secondary)]">
												({Math.round((attacker.damage_done / totalDamage) * 100)}%)
											</span>
										{/if}
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			</section>
		{/if}

		<!-- Items -->
		{#if destroyedItems.length > 0 || droppedItems.length > 0}
			<section class="grid grid-cols-1 gap-6 lg:grid-cols-2">
				{#if destroyedItems.length > 0}
					<div>
						<h2 class="mb-4 text-lg font-semibold text-[var(--color-accent-red)]">Destroyed</h2>
						<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]">
							<div class="space-y-0">
								{#each destroyedItems as item}
									<div class="flex items-center justify-between border-b border-[var(--color-border)] px-4 py-2 last:border-b-0">
										<div class="flex items-center gap-2">
											<img
												src="https://images.evetech.net/types/{item.type_id}/icon?size=32"
												alt={item.type_name ?? 'Unknown'}
												class="h-6 w-6"
											/>
											<span class="text-sm text-[var(--color-text-primary)]">
												{item.type_name ?? `Type ${item.type_id}`}
											</span>
										</div>
										<span class="font-mono text-sm text-[var(--color-accent-red)]">
											x{formatNumber(item.quantity_destroyed)}
										</span>
									</div>
								{/each}
							</div>
						</div>
					</div>
				{/if}

				{#if droppedItems.length > 0}
					<div>
						<h2 class="mb-4 text-lg font-semibold text-[var(--color-accent-green)]">Dropped</h2>
						<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)]">
							<div class="space-y-0">
								{#each droppedItems as item}
									<div class="flex items-center justify-between border-b border-[var(--color-border)] px-4 py-2 last:border-b-0">
										<div class="flex items-center gap-2">
											<img
												src="https://images.evetech.net/types/{item.type_id}/icon?size=32"
												alt={item.type_name ?? 'Unknown'}
												class="h-6 w-6"
											/>
											<span class="text-sm text-[var(--color-text-primary)]">
												{item.type_name ?? `Type ${item.type_id}`}
											</span>
										</div>
										<span class="font-mono text-sm text-[var(--color-accent-green)]">
											x{formatNumber(item.quantity_dropped)}
										</span>
									</div>
								{/each}
							</div>
						</div>
					</div>
				{/if}
			</section>
		{/if}
	{/if}
</div>
