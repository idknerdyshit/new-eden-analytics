<script lang="ts">
	import { onMount } from 'svelte';
	import { select, pointer } from 'd3-selection';
	import { scaleBand, scaleLinear } from 'd3-scale';
	import { axisBottom, axisLeft } from 'd3-axis';
	import { line, curveMonotoneX } from 'd3-shape';
	import { timeParse, timeFormat } from 'd3-time-format';
	import { format } from 'd3-format';
	import { range, max } from 'd3-array';

	interface DestructionDataPoint {
		date: string;
		quantity_destroyed: number;
		kill_count: number;
	}

	interface Props {
		data: DestructionDataPoint[];
	}

	let { data }: Props = $props();

	let container = $state<HTMLDivElement>(undefined!);
	let svgEl = $state<SVGSVGElement>(undefined!);
	let width = $state(0);
	let renderError = $state<string | null>(null);
	const height = 400;
	const margin = { top: 20, right: 30, bottom: 40, left: 60 };

	let resizeObserver: ResizeObserver | undefined;

	function computeMovingAverage(values: DestructionDataPoint[], windowSize: number): (number | null)[] {
		return values.map((_, i) => {
			if (i < windowSize - 1) return null;
			let sum = 0;
			for (let j = i - windowSize + 1; j <= i; j++) {
				sum += values[j].quantity_destroyed;
			}
			return sum / windowSize;
		});
	}

	function render() {
		if (!svgEl || width === 0 || !data || data.length === 0) return;

		const svg = select(svgEl);
		svg.selectAll('*').remove();

		const innerWidth = width - margin.left - margin.right;
		const innerHeight = height - margin.top - margin.bottom;

		const g = svg
			.attr('width', width)
			.attr('height', height)
			.append('g')
			.attr('transform', `translate(${margin.left},${margin.top})`);

		const parseDate = timeParse('%Y-%m-%d');
		const parsed = data.map((d) => ({
			...d,
			parsedDate: parseDate(d.date) ?? new Date(d.date)
		}));

		const x = scaleBand<number>()
			.domain(range(parsed.length))
			.range([0, innerWidth])
			.padding(0.15);

		const yMax = max(parsed, (d) => d.quantity_destroyed) ?? 0;
		const y = scaleLinear().domain([0, yMax * 1.1]).nice().range([innerHeight, 0]);

		// X axis
		const tickInterval = Math.max(1, Math.floor(parsed.length / 8));
		const tickValues = parsed
			.map((_, i) => i)
			.filter((i) => i % tickInterval === 0);
		const xAxis = axisBottom(x).tickValues(tickValues).tickFormat((i) => {
			const d = parsed[i as number];
			return d ? timeFormat('%b %d')(d.parsedDate) : '';
		});
		g.append('g')
			.attr('transform', `translate(0,${innerHeight})`)
			.call(xAxis)
			.selectAll('text')
			.attr('fill', '#8b949e')
			.style('font-size', '11px');
		g.selectAll('.domain, .tick line').attr('stroke', '#8b949e');

		// Y axis
		g.append('g')
			.call(axisLeft(y).ticks(6).tickFormat(format('.2s')))
			.selectAll('text')
			.attr('fill', '#8b949e')
			.style('font-size', '11px');
		g.selectAll('.domain, .tick line').attr('stroke', '#8b949e');

		// Bars
		g.selectAll('.bar')
			.data(parsed)
			.enter()
			.append('rect')
			.attr('class', 'bar')
			.attr('x', (_, i) => x(i) ?? 0)
			.attr('y', (d) => y(d.quantity_destroyed))
			.attr('width', x.bandwidth())
			.attr('height', (d) => innerHeight - y(d.quantity_destroyed))
			.attr('fill', '#f0883e')
			.attr('rx', 1);

		// 7-day moving average
		const ma = computeMovingAverage(data, 7);
		const maData = parsed
			.map((d, i) => ({ ...d, ma: ma[i], index: i }))
			.filter((d) => d.ma !== null) as Array<(typeof parsed)[0] & { ma: number; index: number }>;

		if (maData.length > 1) {
			const maLine = line<(typeof maData)[0]>()
				.x((d) => (x(d.index) ?? 0) + x.bandwidth() / 2)
				.y((d) => y(d.ma))
				.curve(curveMonotoneX);

			g.append('path')
				.datum(maData)
				.attr('fill', 'none')
				.attr('stroke', '#ffffff')
				.attr('stroke-width', 2)
				.attr('d', maLine);
		}

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

		g.selectAll('.bar')
			.on('mouseenter', function (event, d: any) {
				select(this).attr('fill', '#f5a623');
				tooltip
					.style('opacity', 1)
					.text(
						`${d.date}\nDestroyed: ${format(',')(d.quantity_destroyed)}\nKills: ${format(',')(d.kill_count)}`
					);
			})
			.on('mousemove', function (event) {
				const [mx, my] = pointer(event, container);
				tooltip.style('left', mx + 16 + 'px').style('top', my - 10 + 'px');
			})
			.on('mouseleave', function () {
				select(this).attr('fill', '#f0883e');
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
		// Track reactive deps
		data;
		width;
		try {
			renderError = null;
			render();
		} catch (e) {
			console.error('[nea] DestructionChart render failed', e);
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
