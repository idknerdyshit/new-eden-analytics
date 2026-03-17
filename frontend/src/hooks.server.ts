import type { Handle } from '@sveltejs/kit';

const API_BACKEND = process.env.API_BACKEND_URL || 'http://localhost:3001';

export const handle: Handle = async ({ event, resolve }) => {
	if (event.url.pathname.startsWith('/api/')) {
		const backendUrl = `${API_BACKEND}${event.url.pathname}${event.url.search}`;
		const headers = new Headers(event.request.headers);
		headers.delete('host');

		console.info(`[nea] proxy ${event.request.method} ${event.url.pathname}`);
		const start = performance.now();

		let resp: Response;
		try {
			resp = await fetch(backendUrl, {
				method: event.request.method,
				headers,
				body: event.request.method !== 'GET' && event.request.method !== 'HEAD'
					? await event.request.arrayBuffer()
					: undefined,
			});
		} catch (e) {
			const elapsed = Math.round(performance.now() - start);
			console.error(`[nea] proxy ${event.url.pathname} error after ${elapsed}ms`, e);
			return new Response(JSON.stringify({ error: 'Bad Gateway' }), {
				status: 502,
				headers: { 'Content-Type': 'application/json' },
			});
		}

		const elapsed = Math.round(performance.now() - start);
		const requestId = resp.headers.get('x-request-id');
		console.info(`[nea] proxy ${event.url.pathname} -> ${resp.status} (${elapsed}ms${requestId ? `, rid=${requestId}` : ''})`);

		return new Response(resp.body, {
			status: resp.status,
			statusText: resp.statusText,
			headers: resp.headers,
		});
	}

	return resolve(event);
};
