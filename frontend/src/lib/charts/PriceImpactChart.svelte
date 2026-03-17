<script lang="ts">
	import { onMount } from 'svelte';
	import * as d3 from 'd3';

	interface PriceDataPoint {
		date: string;
		average: number;
	}

	interface DestructionDataPoint {
		date: string;
		quantity_destroyed: number;
	}

	interface Props {
		priceData: PriceDataPoint[];
		destructionData: DestructionDataPoint[];
		materialName: string;
	}

	let { priceData, destructionData, materialName }: Props = $props();

	let container = $state<HTMLDivElement>(undefined!);
	let svgEl = $state<SVGSVGElement>(undefined!);
	let width = $state(0);
	let renderError = $state<string | null>(null);
	const height = 420;
	const margin = { top: 20, right: 60, bottom: 40, left: 60 };

	let resizeObserver: ResizeObserver | undefined;

	function render() {
		if (!svgEl || width === 0) return;
		const hasPrice = priceData && priceData.length > 0;
		const hasDestruction = destructionData && destructionData.length > 0;
		if (!hasPrice && !hasDestruction) return;

		const svg = d3.select(svgEl);
		svg.selectAll('*').remove();

		const innerWidth = width - margin.left - margin.right;
		const innerHeight = height - margin.top - margin.bottom;

		const parseDate = d3.timeParse('%Y-%m-%d');
		const priceParsed = (priceData ?? []).map((d) => ({
			...d,
			parsedDate: parseDate(d.date) ?? new Date(d.date)
		}));
		const destructionParsed = (destructionData ?? []).map((d) => ({
			...d,
			parsedDate: parseDate(d.date) ?? new Date(d.date)
		}));

		// Collect all dates for shared X domain
		const allDates = [
			...priceParsed.map((d) => d.parsedDate),
			...destructionParsed.map((d) => d.parsedDate)
		];
		const xFullDomain = d3.extent(allDates) as [Date, Date];

		// Clip path
		const clipId = 'clip-' + Math.random().toString(36).slice(2, 9);
		svg
			.attr('width', width)
			.attr('height', height)
			.append('defs')
			.append('clipPath')
			.attr('id', clipId)
			.append('rect')
			.attr('width', innerWidth)
			.attr('height', innerHeight);

		const g = svg.append('g').attr('transform', `translate(${margin.left},${margin.top})`);

		// Scales
		const x = d3.scaleTime().domain(xFullDomain).range([0, innerWidth]);
		const yPrice = d3
			.scaleLinear()
			.domain([0, (d3.max(priceParsed, (d) => d.average) ?? 0) * 1.1])
			.nice()
			.range([innerHeight, 0]);
		const yDestruction = d3
			.scaleLinear()
			.domain([0, (d3.max(destructionParsed, (d) => d.quantity_destroyed) ?? 0) * 1.1])
			.nice()
			.range([innerHeight, 0]);

		// Chart area with clip
		const chartArea = g.append('g').attr('clip-path', `url(#${clipId})`);

		// Destruction area
		if (destructionParsed.length > 0) {
			const area = d3
				.area<(typeof destructionParsed)[0]>()
				.x((d) => x(d.parsedDate))
				.y0(innerHeight)
				.y1((d) => yDestruction(d.quantity_destroyed))
				.curve(d3.curveMonotoneX);

			chartArea
				.append('path')
				.datum(destructionParsed)
				.attr('fill', 'rgba(240,136,62,0.3)')
				.attr('d', area);
		}

		// Price line
		if (priceParsed.length > 0) {
			const line = d3
				.line<(typeof priceParsed)[0]>()
				.x((d) => x(d.parsedDate))
				.y((d) => yPrice(d.average))
				.curve(d3.curveMonotoneX);

			chartArea
				.append('path')
				.datum(priceParsed)
				.attr('fill', 'none')
				.attr('stroke', '#58a6ff')
				.attr('stroke-width', 2)
				.attr('d', line);
		}

		// X axis
		const xAxisG = g
			.append('g')
			.attr('class', 'x-axis')
			.attr('transform', `translate(0,${innerHeight})`)
			.call(d3.axisBottom(x).ticks(6).tickFormat(d3.timeFormat('%b %d') as any));
		xAxisG.selectAll('text').attr('fill', '#8b949e').style('font-size', '11px');
		xAxisG.selectAll('.domain, .tick line').attr('stroke', '#8b949e');

		// Left Y axis (price)
		const yLeftG = g.append('g').call(d3.axisLeft(yPrice).ticks(6).tickFormat(d3.format('.2s')));
		yLeftG.selectAll('text').attr('fill', '#58a6ff').style('font-size', '11px');
		yLeftG.selectAll('.domain, .tick line').attr('stroke', '#8b949e');
		g.append('text')
			.attr('transform', 'rotate(-90)')
			.attr('y', -margin.left + 14)
			.attr('x', -innerHeight / 2)
			.attr('text-anchor', 'middle')
			.attr('fill', '#58a6ff')
			.style('font-size', '12px')
			.text('Price (ISK)');

		// Right Y axis (destruction)
		const yRightG = g
			.append('g')
			.attr('transform', `translate(${innerWidth},0)`)
			.call(d3.axisRight(yDestruction).ticks(6).tickFormat(d3.format('.2s')));
		yRightG.selectAll('text').attr('fill', '#f0883e').style('font-size', '11px');
		yRightG.selectAll('.domain, .tick line').attr('stroke', '#8b949e');
		g.append('text')
			.attr('transform', 'rotate(90)')
			.attr('y', -width + margin.left + 14)
			.attr('x', innerHeight / 2)
			.attr('text-anchor', 'middle')
			.attr('fill', '#f0883e')
			.style('font-size', '12px')
			.text('Destruction');

		// Tooltip
		const tooltip = d3
			.select(container)
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
			.style('opacity', 0)
			.style('z-index', 10);

		// Hover overlay
		const bisectDate = d3.bisector<(typeof priceParsed)[0], Date>((d) => d.parsedDate).left;
		const overlay = chartArea
			.append('rect')
			.attr('width', innerWidth)
			.attr('height', innerHeight)
			.attr('fill', 'none')
			.attr('pointer-events', 'all');

		const hoverLine = chartArea
			.append('line')
			.attr('stroke', '#8b949e')
			.attr('stroke-width', 1)
			.attr('stroke-dasharray', '4,3')
			.attr('y1', 0)
			.attr('y2', innerHeight)
			.style('opacity', 0);

		overlay
			.on('mouseenter', () => {
				tooltip.style('opacity', 1);
				hoverLine.style('opacity', 1);
			})
			.on('mousemove', function (event) {
				const [mx, my] = d3.pointer(event, container);
				const xPos = d3.pointer(event, this)[0];
				const dateAtMouse = x.invert(xPos);

				let priceVal = '';
				if (priceParsed.length > 0) {
					const idx = Math.min(
						bisectDate(priceParsed, dateAtMouse),
						priceParsed.length - 1
					);
					const p = priceParsed[idx];
					if (p) priceVal = `Price: ${d3.format(',.0f')(p.average)} ISK<br/>`;
				}

				let destVal = '';
				if (destructionParsed.length > 0) {
					const bisect2 = d3.bisector<(typeof destructionParsed)[0], Date>(
						(d) => d.parsedDate
					).left;
					const idx = Math.min(
						bisect2(destructionParsed, dateAtMouse),
						destructionParsed.length - 1
					);
					const dd = destructionParsed[idx];
					if (dd)
						destVal = `Destroyed: ${d3.format(',')(dd.quantity_destroyed)}<br/>`;
				}

				hoverLine.attr('x1', xPos).attr('x2', xPos);
				tooltip
					.html(
						`<strong>${d3.timeFormat('%Y-%m-%d')(dateAtMouse)}</strong><br/>` +
							priceVal +
							destVal
					)
					.style('left', mx + 16 + 'px')
					.style('top', my - 10 + 'px');
			})
			.on('mouseleave', () => {
				tooltip.style('opacity', 0);
				hoverLine.style('opacity', 0);
			});

		// Brush for zoom
		const brush = d3
			.brushX()
			.extent([
				[0, 0],
				[innerWidth, innerHeight]
			])
			.on('end', (event) => {
				if (!event.selection) {
					// Reset zoom
					x.domain(xFullDomain);
					updateChart();
					return;
				}
				const [x0, x1] = event.selection as [number, number];
				x.domain([x.invert(x0), x.invert(x1)]);
				chartArea.select<SVGGElement>('.brush').call(brush.move, null);
				updateChart();
			});

		chartArea.append('g').attr('class', 'brush').call(brush);

		function updateChart() {
			// Re-render clipped content
			chartArea.selectAll('path').remove();
			chartArea.selectAll('line.hover-line').remove();

			if (destructionParsed.length > 0) {
				const area = d3
					.area<(typeof destructionParsed)[0]>()
					.x((d) => x(d.parsedDate))
					.y0(innerHeight)
					.y1((d) => yDestruction(d.quantity_destroyed))
					.curve(d3.curveMonotoneX);
				chartArea.insert('path', '.brush').datum(destructionParsed).attr('fill', 'rgba(240,136,62,0.3)').attr('d', area);
			}
			if (priceParsed.length > 0) {
				const line = d3
					.line<(typeof priceParsed)[0]>()
					.x((d) => x(d.parsedDate))
					.y((d) => yPrice(d.average))
					.curve(d3.curveMonotoneX);
				chartArea
					.insert('path', '.brush')
					.datum(priceParsed)
					.attr('fill', 'none')
					.attr('stroke', '#58a6ff')
					.attr('stroke-width', 2)
					.attr('d', line);
			}
			xAxisG.call(d3.axisBottom(x).ticks(6).tickFormat(d3.timeFormat('%b %d') as any));
			xAxisG.selectAll('text').attr('fill', '#8b949e').style('font-size', '11px');
			xAxisG.selectAll('.domain, .tick line').attr('stroke', '#8b949e');
		}
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
			d3.select(container).selectAll('.chart-tooltip').remove();
		};
	});

	$effect(() => {
		priceData;
		destructionData;
		materialName;
		width;
		try {
			renderError = null;
			render();
		} catch (e) {
			console.error('[nea] PriceImpactChart render failed', e);
			renderError = 'Chart rendering failed';
		}
	});
</script>

<div bind:this={container} class="relative w-full">
	{#if renderError}
		<div class="flex items-center justify-center text-[var(--color-accent-red)]" style="height: {height}px">
			{renderError}
		</div>
	{:else if (!priceData || priceData.length === 0) && (!destructionData || destructionData.length === 0)}
		<div class="flex items-center justify-center text-[#8b949e]" style="height: {height}px">
			No data available
		</div>
	{:else}
		<svg bind:this={svgEl}></svg>
	{/if}
</div>
