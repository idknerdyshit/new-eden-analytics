<script lang="ts">
	import { onMount } from 'svelte';
	import * as d3 from 'd3';

	interface CorrelationEntry {
		material_name: string;
		lag_days: number;
		correlation_coeff: number;
		granger_significant: boolean;
	}

	interface Props {
		correlations: CorrelationEntry[];
	}

	let { correlations }: Props = $props();

	let container = $state<HTMLDivElement>(undefined!);
	let svgEl = $state<SVGSVGElement>(undefined!);
	let width = $state(0);
	let renderError = $state<string | null>(null);
	const rowHeight = 44;
	const margin = { top: 20, right: 30, bottom: 40, left: 160 };

	let resizeObserver: ResizeObserver | undefined;

	function render() {
		if (!svgEl || width === 0 || !correlations || correlations.length === 0) return;

		const svg = d3.select(svgEl);
		svg.selectAll('*').remove();

		const innerWidth = width - margin.left - margin.right;
		const height = margin.top + correlations.length * rowHeight + margin.bottom;
		const innerHeight = correlations.length * rowHeight;

		svg.attr('width', width).attr('height', height);
		const g = svg.append('g').attr('transform', `translate(${margin.left},${margin.top})`);

		// Sort by lag days
		const sorted = [...correlations].sort((a, b) => a.lag_days - b.lag_days);

		const maxLag = d3.max(sorted, (d) => Math.abs(d.lag_days)) ?? 1;

		const x = d3.scaleLinear().domain([0, maxLag * 1.15]).range([0, innerWidth]);

		// Color scale based on correlation strength
		const colorScale = d3
			.scaleSequential(d3.interpolateYlOrRd)
			.domain([0, 1]);

		// Rows
		sorted.forEach((d, i) => {
			const y = i * rowHeight + rowHeight / 2;

			// Material label
			g.append('text')
				.attr('x', -10)
				.attr('y', y + 4)
				.attr('text-anchor', 'end')
				.attr('fill', '#e6edf3')
				.style('font-size', '13px')
				.text(d.material_name.length > 20 ? d.material_name.slice(0, 18) + '...' : d.material_name);

			// Row background
			g.append('rect')
				.attr('x', 0)
				.attr('y', y - rowHeight / 2 + 4)
				.attr('width', innerWidth)
				.attr('height', rowHeight - 8)
				.attr('fill', i % 2 === 0 ? 'rgba(255,255,255,0.02)' : 'rgba(255,255,255,0.04)')
				.attr('rx', 3);

			const absCorr = Math.abs(d.correlation_coeff);
			const barEnd = x(Math.abs(d.lag_days));

			// Arrow line
			g.append('line')
				.attr('x1', 0)
				.attr('x2', barEnd)
				.attr('y1', y)
				.attr('y2', y)
				.attr('stroke', colorScale(absCorr))
				.attr('stroke-width', 3)
				.attr('stroke-opacity', 0.5 + absCorr * 0.5);

			// Arrowhead
			if (barEnd > 10) {
				g.append('polygon')
					.attr(
						'points',
						`${barEnd},${y} ${barEnd - 8},${y - 5} ${barEnd - 8},${y + 5}`
					)
					.attr('fill', colorScale(absCorr))
					.attr('fill-opacity', 0.6 + absCorr * 0.4);
			}

			// Lag label on arrow
			g.append('text')
				.attr('x', Math.max(barEnd / 2, 20))
				.attr('y', y - 8)
				.attr('text-anchor', 'middle')
				.attr('fill', '#e6edf3')
				.style('font-size', '11px')
				.style('font-weight', '600')
				.text(`${d.lag_days}d`);

			// Correlation coefficient
			g.append('text')
				.attr('x', barEnd + 8)
				.attr('y', y + 4)
				.attr('text-anchor', 'start')
				.attr('fill', '#8b949e')
				.style('font-size', '11px')
				.text(`r=${d3.format('.3f')(d.correlation_coeff)}`);

			// Granger significance dot
			if (d.granger_significant) {
				g.append('circle')
					.attr('cx', barEnd + 70)
					.attr('cy', y)
					.attr('r', 5)
					.attr('fill', '#3fb950');
				g.append('text')
					.attr('x', barEnd + 80)
					.attr('y', y + 4)
					.attr('fill', '#3fb950')
					.style('font-size', '10px')
					.text('Granger');
			}
		});

		// X axis
		const xAxisG = g
			.append('g')
			.attr('transform', `translate(0,${innerHeight})`)
			.call(d3.axisBottom(x).ticks(6));
		xAxisG.selectAll('text').attr('fill', '#8b949e').style('font-size', '11px');
		xAxisG.selectAll('.domain, .tick line').attr('stroke', '#8b949e');
		g.append('text')
			.attr('x', innerWidth / 2)
			.attr('y', innerHeight + 35)
			.attr('text-anchor', 'middle')
			.attr('fill', '#8b949e')
			.style('font-size', '12px')
			.text('Lag (days): Destruction \u2192 Price Effect');

		// Legend
		const legend = g.append('g').attr('transform', `translate(${innerWidth - 120}, -10)`);
		legend.append('circle').attr('cx', 0).attr('cy', 0).attr('r', 5).attr('fill', '#3fb950');
		legend
			.append('text')
			.attr('x', 10)
			.attr('y', 4)
			.attr('fill', '#8b949e')
			.style('font-size', '11px')
			.text('Granger significant');
	}

	onMount(() => {
		resizeObserver = new ResizeObserver((entries) => {
			for (const entry of entries) {
				width = entry.contentRect.width;
			}
		});
		resizeObserver.observe(container);
		return () => {
			resizeObserver?.disconnect();
		};
	});

	$effect(() => {
		correlations;
		width;
		try {
			renderError = null;
			render();
		} catch (e) {
			console.error('[nea] LagTimeline render failed', e);
			renderError = 'Chart rendering failed';
		}
	});
</script>

<div bind:this={container} class="relative w-full">
	{#if renderError}
		<div class="flex items-center justify-center text-[var(--color-accent-red)]" style="height: 200px">
			{renderError}
		</div>
	{:else if !correlations || correlations.length === 0}
		<div class="flex items-center justify-center text-[#8b949e]" style="height: 200px">
			No data available
		</div>
	{:else}
		<svg bind:this={svgEl}></svg>
	{/if}
</div>
