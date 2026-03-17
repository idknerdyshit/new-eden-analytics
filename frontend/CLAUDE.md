# Frontend — SvelteKit

## Stack

- SvelteKit 2 with `@sveltejs/adapter-node` (SSR)
- Svelte 5 (Runes mode enabled globally via `vite.config.ts`)
- TypeScript 5.9
- Tailwind CSS v4 (via `@tailwindcss/vite` plugin)
- D3.js v7 (chart rendering)

## Commands

```bash
npm run dev       # dev server on :5173, proxies /api to localhost:3001
npm run check     # svelte-check + TypeScript
npm run build     # production build
```

## Routes

| Path | Description |
|------|-------------|
| `/` | Dashboard — top correlations + top destruction |
| `/search` | Item search with pagination |
| `/items/[typeId]` | Item detail — market data, destruction, correlations, charts |
| `/auth/callback` | EVE SSO OAuth callback handler |

## API Client

`src/lib/api/client.ts`:
- `fetchJson<T>` wrapper with `performance.now()` timing, `[nea]` console logging, `x-request-id` capture
- `api` object with typed methods: `dashboard`, `movers`, `searchItems`, `getItem`, `marketHistory`, `marketSnapshots`, `correlations`, `topCorrelations`, `destruction`, `authMe`
- All requests go to `/api/*` (proxied to backend)

## Server Hooks

`src/hooks.server.ts`:
- Proxies all `/api/*` requests to `API_BACKEND_URL` (default `http://localhost:3001`)
- Returns 502 JSON on backend connection failure
- Logs proxy timing and request IDs with `[nea]` prefix

## Stores

- `src/lib/stores/auth.ts` — user state (writable)
- `src/lib/stores/search.ts` — search query + results

Stores are minimal; most state is component-local via `$state`.

## Svelte 5 Patterns

- Use `$state`, `$derived`, `$effect` for reactivity (not stores for local state)
- Use `$props` for component inputs
- Use `{@render children()}` for slot content
- Runes mode is enabled globally — do not use `$:` reactive statements

## Charts

4 D3 chart components in `src/lib/charts/`:
- `DestructionChart.svelte` — destruction volume over time
- `PriceImpactChart.svelte` — price impact visualization
- `CorrelationChart.svelte` — correlation coefficients
- `LagTimeline.svelte` — lag timeline display

Pattern: manual SVG rendering with D3 scales, ResizeObserver for responsive sizing, `renderError` state variable for error boundaries.

## Styling

Tailwind v4 utilities + CSS custom properties in `src/app.css`:

```
--color-bg-primary: #0d1117     (dark background)
--color-bg-secondary: #161b22
--color-bg-tertiary: #21262d
--color-text-primary: #e6edf3
--color-text-secondary: #8b949e
--color-accent-blue: #58a6ff
--color-accent-green: #3fb950
--color-accent-red: #f85149
--color-border: #30363d
```

Dark theme only. System font stack.

## Error Handling

Pattern: try/catch in `onMount` or load functions, `console.error('[nea] ...')`, error displayed via `$state` variable in the component UI.

## Testing

No tests configured.
