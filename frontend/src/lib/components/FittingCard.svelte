<script lang="ts">
	import type { FittingCluster, FittingModule } from '$lib/api/client';

	let { fitting }: { fitting: FittingCluster } = $props();

	function slotLabel(flag: number): string {
		if (flag >= 27 && flag <= 34) return 'High';
		if (flag >= 19 && flag <= 26) return 'Mid';
		if (flag >= 11 && flag <= 18) return 'Low';
		if (flag >= 92 && flag <= 94) return 'Rig';
		if (flag >= 125 && flag <= 131) return 'Sub';
		return '?';
	}

	function slotColor(flag: number): string {
		if (flag >= 27 && flag <= 34) return 'var(--color-accent-red)';
		if (flag >= 19 && flag <= 26) return 'var(--color-accent-blue)';
		if (flag >= 11 && flag <= 18) return 'var(--color-accent-green)';
		if (flag >= 92 && flag <= 94) return '#d29922';
		if (flag >= 125 && flag <= 131) return '#a371f7';
		return 'var(--color-text-secondary)';
	}

	function groupBySlot(modules: FittingModule[]): Map<string, FittingModule[]> {
		const groups = new Map<string, FittingModule[]>();
		const order = ['High', 'Mid', 'Low', 'Rig', 'Sub'];
		for (const label of order) {
			groups.set(label, []);
		}
		for (const mod of modules) {
			const label = slotLabel(mod.flag);
			const list = groups.get(label);
			if (list) list.push(mod);
		}
		return groups;
	}

	let slotGroups = $derived(groupBySlot(fitting.modules));
</script>

<div class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-4">
	<div class="mb-3 flex items-center justify-between">
		<div class="flex items-center gap-2">
			<img
				src="https://images.evetech.net/types/{fitting.ship_type_id}/icon?size=32"
				alt={fitting.ship_name}
				class="h-6 w-6"
			/>
			<span class="font-medium text-[var(--color-text-primary)]">{fitting.ship_name}</span>
		</div>
		{#if fitting.count > 0}
			<div class="flex items-center gap-2 text-xs text-[var(--color-text-secondary)]">
				<span>{fitting.count} loss{fitting.count !== 1 ? 'es' : ''}</span>
				{#if fitting.variant_count > 1}
					<span class="rounded bg-[var(--color-bg-tertiary)] px-1.5 py-0.5">
						{fitting.variant_count} variants
					</span>
				{/if}
			</div>
		{/if}
	</div>

	<div class="space-y-2">
		{#each [...slotGroups.entries()] as [label, modules]}
			{#if modules.length > 0}
				<div>
					<div class="mb-1 text-xs font-medium" style="color: {slotColor(modules[0].flag)}">
						{label} Slots
					</div>
					<div class="space-y-0.5">
						{#each modules as mod}
							<div class="flex items-center gap-2 text-sm text-[var(--color-text-secondary)]">
								<img
									src="https://images.evetech.net/types/{mod.type_id}/icon?size=32"
									alt={mod.name}
									class="h-4 w-4"
								/>
								<span>{mod.name}</span>
							</div>
						{/each}
					</div>
				</div>
			{/if}
		{/each}
	</div>
</div>
