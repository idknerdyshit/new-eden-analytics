export function formatCorrelation(value: number): string {
	return value.toFixed(4);
}

export function correlationColor(value: number): string {
	if (value > 0) return 'text-[var(--color-accent-blue)]';
	if (value < 0) return 'text-[var(--color-accent-red)]';
	return 'text-[var(--color-text-secondary)]';
}

export function formatNumber(value: number): string {
	if (value >= 1_000_000_000) return (value / 1_000_000_000).toFixed(1) + 'B';
	if (value >= 1_000_000) return (value / 1_000_000).toFixed(1) + 'M';
	if (value >= 1_000) return (value / 1_000).toFixed(1) + 'K';
	return value.toLocaleString();
}

export function formatPrice(value: number): string {
	return value.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

export function formatPValue(value: number | null): string {
	if (value === null) return '--';
	if (value < 0.001) return '< 0.001';
	return value.toFixed(4);
}

export function changeColor(pct: number): string {
	if (pct > 0) return 'text-[var(--color-accent-green)]';
	if (pct < 0) return 'text-[var(--color-accent-red)]';
	return 'text-[var(--color-text-secondary)]';
}

export function changeArrow(pct: number): string {
	if (pct > 0) return '+';
	return '';
}
