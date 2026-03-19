<script lang="ts">
	import { onMount } from 'svelte';
	import { select, pointer } from 'd3-selection';
	import { scaleBand, scaleLinear } from 'd3-scale';
	import { axisBottom, axisLeft } from 'd3-axis';
	import { format } from 'd3-format';

	interface CCFDataPoint {
		lag: number;
		correlation: number;
	}

	interface Props {
		data: CCFDataPoint[];
		optimalLag: number;
		confidenceThreshold: number;
	}

	let { data, optimalLag, confidenceThreshold }: Props = $props();

	let container = $state<HTMLDivElement>(undefined!);
	let svgEl = $state<SVGSVGElement>(undefined!);
	let width = $state(0);
	let renderError = $state<string | null>(null);
	const height = 360;
	const margin = { top: 20, right: 30, bottom: 40, left: 60 };

	let resizeObserver: ResizeObserver | undefined;

	function render() {
		if (!svgEl || width === 0 || !data || data.length === 0) return;

		const svg = select(svgEl);
		svg.selectAll('*').remove();

		const innerWidth = width - margin.left - margin.right;
		const innerHeight = height - margin.top - margin.bottom;

		svg.attr('width', width).attr('height', height);
		const g = svg.append('g').attr('transform', `translate(${margin.left},${margin.top})`);

		// Scales
		const x = scaleBand<number>()
			.domain(data.map((d) => d.lag))
			.range([0, innerWidth])
			.padding(0.2);

		const y = scaleLinear().domain([-1, 1]).range([innerHeight, 0]);

		// Zero line
		g.append('line')
			.attr('x1', 0)
			.attr('x2', innerWidth)
			.attr('y1', y(0))
			.attr('y2', y(0))
			.attr('stroke', '#8b949e')
			.attr('stroke-width', 1);

		// Confidence threshold lines
		if (confidenceThreshold > 0) {
			[confidenceThreshold, -confidenceThreshold].forEach((val) => {
				g.append('line')
					.attr('x1', 0)
					.attr('x2', innerWidth)
					.attr('y1', y(val))
					.attr('y2', y(val))
					.attr('stroke', '#8b949e')
					.attr('stroke-width', 1)
					.attr('stroke-dasharray', '6,4');
			});
		}

		// Bars
		g.selectAll('.ccf-bar')
			.data(data)
			.enter()
			.append('rect')
			.attr('class', 'ccf-bar')
			.attr('x', (d) => x(d.lag) ?? 0)
			.attr('width', x.bandwidth())
			.attr('y', (d) => (d.correlation >= 0 ? y(d.correlation) : y(0)))
			.attr('height', (d) => Math.abs(y(d.correlation) - y(0)))
			.attr('fill', (d) => {
				if (d.lag === optimalLag) return d.correlation >= 0 ? '#79c0ff' : '#ff7b72';
				return d.correlation >= 0 ? '#58a6ff' : '#da3633';
			})
			.attr('stroke', (d) => (d.lag === optimalLag ? '#ffffff' : 'none'))
			.attr('stroke-width', (d) => (d.lag === optimalLag ? 2 : 0))
			.attr('rx', 1);

		// X axis
		const tickEvery = Math.max(1, Math.floor(data.length / 12));
		const xTickValues = data.map((d) => d.lag).filter((_, i) => i % tickEvery === 0);
		g.append('g')
			.attr('transform', `translate(0,${innerHeight})`)
			.call(axisBottom(x).tickValues(xTickValues).tickFormat((d) => `${d}`))
			.selectAll('text')
			.attr('fill', '#8b949e')
			.style('font-size', '11px');
		g.append('text')
			.attr('x', innerWidth / 2)
			.attr('y', innerHeight + 35)
			.attr('text-anchor', 'middle')
			.attr('fill', '#8b949e')
			.style('font-size', '12px')
			.text('Lag (days)');

		// Y axis
		g.append('g')
			.call(axisLeft(y).ticks(8).tickFormat(format('.2f')))
			.selectAll('text')
			.attr('fill', '#8b949e')
			.style('font-size', '11px');

		g.selectAll('.domain, .tick line').attr('stroke', '#8b949e');

		// Tooltip
		const tooltip = select(container)
			.selectAll<HTMLDivElement, unknown>('.chart-tooltip')
			.data([null])
			.join('div')
			.attr('class', 'chart-tooltip')
			.style('position', 'absolute')
			.style('pointer-events', 'none')
			.style('background', 'rgba(22, 27, 34, 0.95)')
			.style('border', '1px solid #30363d')
			.style('border-radius', '6px')
			.style('padding', '8px 12px')
			.style('color', '#e6edf3')
			.style('font-size', '12px')
			.style('white-space', 'pre-line')
			.style('opacity', 0)
			.style('z-index', 10);

		g.selectAll('.ccf-bar')
			.on('mouseenter', function (event, d: any) {
				select(this).style('opacity', 0.8);
				tooltip
					.style('opacity', 1)
					.text(
						`Lag: ${d.lag} days\nCorrelation: ${format('.4f')(d.correlation)}${d.lag === optimalLag ? '\nOptimal lag' : ''}`
					);
			})
			.on('mousemove', function (event) {
				const [mx, my] = pointer(event, container);
				tooltip.style('left', mx + 16 + 'px').style('top', my - 10 + 'px');
			})
			.on('mouseleave', function () {
				select(this).style('opacity', 1);
				tooltip.style('opacity', 0);
			});
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
			select(container).selectAll('.chart-tooltip').remove();
		};
	});

	$effect(() => {
		data;
		optimalLag;
		confidenceThreshold;
		width;
		try {
			renderError = null;
			render();
		} catch (e) {
			console.error('[nea] CorrelationChart render failed', e);
			renderError = 'Chart rendering failed';
		}
	});
</script>

<div bind:this={container} class="relative w-full">
	{#if renderError}
		<div class="flex items-center justify-center text-[var(--color-accent-red)]" style="height: {height}px">
			{renderError}
		</div>
	{:else if !data || data.length === 0}
		<div class="flex items-center justify-center text-[#8b949e]" style="height: {height}px">
			No data available
		</div>
	{:else}
		<svg bind:this={svgEl}></svg>
	{/if}
</div>
