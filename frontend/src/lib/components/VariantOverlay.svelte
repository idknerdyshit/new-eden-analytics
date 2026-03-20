<script lang="ts">
	import type { FittingModule } from '$lib/api/client';

	let {
		ship_type_id,
		ship_name,
		canonical_fit,
		variants,
		onclose
	}: {
		ship_type_id: number;
		ship_name: string;
		canonical_fit: FittingModule[];
		variants: FittingModule[][];
		onclose: () => void;
	} = $props();

	let selectedVariant = $state(0);

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

	const slotOrder = ['High', 'Mid', 'Low', 'Rig', 'Sub'];

	function groupBySlot(modules: FittingModule[]): Map<string, FittingModule[]> {
		const groups = new Map<string, FittingModule[]>();
		for (const label of slotOrder) groups.set(label, []);
		for (const mod of modules) {
			const label = slotLabel(mod.flag);
			groups.get(label)?.push(mod);
		}
		return groups;
	}

	/** Build a multiset of type_ids from a module list (counts duplicates). */
	function moduleMultiset(modules: FittingModule[]): Map<number, number> {
		const m = new Map<number, number>();
		for (const mod of modules) {
			m.set(mod.type_id, (m.get(mod.type_id) ?? 0) + 1);
		}
		return m;
	}

	type DiffStatus = 'same' | 'added' | 'removed';

	interface DiffModule {
		type_id: number;
		name: string;
		flag: number;
		status: DiffStatus;
	}

	/**
	 * Compute a per-slot diff between canonical and variant.
	 * Returns modules annotated with diff status.
	 */
	function computeDiff(
		canonical: FittingModule[],
		variant: FittingModule[]
	): Map<string, DiffModule[]> {
		const result = new Map<string, DiffModule[]>();
		for (const label of slotOrder) result.set(label, []);

		const canonBySlot = groupBySlot(canonical);
		const varBySlot = groupBySlot(variant);

		for (const label of slotOrder) {
			const canonMods = canonBySlot.get(label) ?? [];
			const varMods = varBySlot.get(label) ?? [];

			const canonCounts = moduleMultiset(canonMods);
			const varCounts = moduleMultiset(varMods);
			const diffMods: DiffModule[] = [];

			// Modules in variant: mark as 'same' if also in canonical, 'added' otherwise
			const usedCanon = new Map<number, number>();
			for (const mod of varMods) {
				const canonRemaining =
					(canonCounts.get(mod.type_id) ?? 0) - (usedCanon.get(mod.type_id) ?? 0);
				if (canonRemaining > 0) {
					diffMods.push({ ...mod, status: 'same' });
					usedCanon.set(mod.type_id, (usedCanon.get(mod.type_id) ?? 0) + 1);
				} else {
					diffMods.push({ ...mod, status: 'added' });
				}
			}

			// Modules in canonical but not accounted for in variant: 'removed'
			for (const mod of canonMods) {
				const used = usedCanon.get(mod.type_id) ?? 0;
				if (used > 0) {
					usedCanon.set(mod.type_id, used - 1);
				} else {
					diffMods.push({ ...mod, status: 'removed' });
				}
			}

			result.set(label, diffMods);
		}
		return result;
	}

	let currentDiff = $derived(computeDiff(canonical_fit, variants[selectedVariant]));

	function handleBackdropClick(e: MouseEvent) {
		if (e.target === e.currentTarget) onclose();
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') onclose();
	}
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- Backdrop -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
	onclick={handleBackdropClick}
>
	<div
		class="relative max-h-[85vh] w-full max-w-2xl overflow-y-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-primary)] shadow-xl"
	>
		<!-- Header -->
		<div
			class="sticky top-0 z-10 flex items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-bg-primary)] p-4"
		>
			<div class="flex items-center gap-2">
				<img
					src="https://images.evetech.net/types/{ship_type_id}/icon?size=32"
					alt={ship_name}
					class="h-6 w-6"
				/>
				<span class="font-medium text-[var(--color-text-primary)]">{ship_name}</span>
				<span class="text-sm text-[var(--color-text-secondary)]">— Fit Variants</span>
			</div>
			<button
				class="rounded p-1 text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-tertiary)] hover:text-[var(--color-text-primary)]"
				onclick={onclose}
				aria-label="Close"
			>
				<svg class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
					<path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
				</svg>
			</button>
		</div>

		<!-- Variant tabs -->
		{#if variants.length > 1}
			<div class="flex gap-1 border-b border-[var(--color-border)] px-4 py-2 overflow-x-auto">
				{#each variants as _, i}
					<button
						class="shrink-0 rounded px-3 py-1 text-xs font-medium transition-colors {selectedVariant === i
							? 'bg-[var(--color-accent-blue)] text-white'
							: 'text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-tertiary)] hover:text-[var(--color-text-primary)]'}"
						onclick={() => (selectedVariant = i)}
					>
						Variant {i + 1}
					</button>
				{/each}
			</div>
		{/if}

		<!-- Diff view -->
		<div class="grid grid-cols-2 gap-4 p-4">
			<!-- Canonical column -->
			<div>
				<div class="mb-3 text-xs font-semibold uppercase tracking-wide text-[var(--color-text-secondary)]">
					Canonical Fit
				</div>
				{#each [...groupBySlot(canonical_fit).entries()] as [label, modules]}
					{#if modules.length > 0}
						<div class="mb-2">
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

			<!-- Variant column with diff highlighting -->
			<div>
				<div class="mb-3 text-xs font-semibold uppercase tracking-wide text-[var(--color-text-secondary)]">
					Variant {selectedVariant + 1}
				</div>
				{#each [...currentDiff.entries()] as [label, modules]}
					{#if modules.length > 0}
						<div class="mb-2">
							<div class="mb-1 text-xs font-medium" style="color: {slotColor(modules[0].flag)}">
								{label} Slots
							</div>
							<div class="space-y-0.5">
								{#each modules as mod}
									<div
										class="flex items-center gap-2 rounded px-1 text-sm {mod.status === 'added'
											? 'bg-[color-mix(in_srgb,var(--color-accent-green)_15%,transparent)] text-[var(--color-accent-green)]'
											: mod.status === 'removed'
												? 'bg-[color-mix(in_srgb,var(--color-accent-red)_15%,transparent)] text-[var(--color-accent-red)] line-through'
												: 'text-[var(--color-text-secondary)]'}"
									>
										<img
											src="https://images.evetech.net/types/{mod.type_id}/icon?size=32"
											alt={mod.name}
											class="h-4 w-4 {mod.status === 'removed' ? 'opacity-50' : ''}"
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

		<!-- Legend -->
		<div class="flex items-center gap-4 border-t border-[var(--color-border)] px-4 py-2 text-xs text-[var(--color-text-secondary)]">
			<span class="flex items-center gap-1">
				<span class="inline-block h-2 w-2 rounded-full bg-[var(--color-accent-green)]"></span>
				Added
			</span>
			<span class="flex items-center gap-1">
				<span class="inline-block h-2 w-2 rounded-full bg-[var(--color-accent-red)]"></span>
				Removed
			</span>
		</div>
	</div>
</div>
