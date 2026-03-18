<script lang="ts">
	import '../app.css';
	import favicon from '$lib/assets/favicon.svg';
	import { api } from '$lib/api/client';
	import type { User } from '$lib/api/client';
	import { user } from '$lib/stores/auth';
	import { onMount } from 'svelte';
	import { page } from '$app/stores';

	let { children } = $props();

	let currentUser = $state<User | null>(null);
	let authError = $state<string | null>(null);

	onMount(async () => {
		try {
			const me = await api.authMe();
			currentUser = me;
			user.set(me);
		} catch (e) {
			console.warn('[nea] auth check failed', e);
			currentUser = null;
			user.set(null);
			authError = 'Could not connect to backend';
		}
	});

	function logout() {
		currentUser = null;
		user.set(null);
		window.location.href = '/api/auth/logout';
	}
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
	<title>New Eden Analytics</title>
</svelte:head>

<nav class="border-b border-[var(--color-border)] bg-[var(--color-bg-secondary)]">
	<div class="mx-auto flex max-w-[1280px] items-center justify-between px-6 py-3">
		<div class="flex items-center gap-8">
			<a href="/" class="text-lg font-bold tracking-wide text-[var(--color-text-primary)] no-underline hover:no-underline">
				New Eden Analytics
			</a>
			<div class="flex items-center gap-6">
				<a
					href="/"
					class="text-sm font-medium text-[var(--color-text-secondary)] no-underline transition-colors hover:text-[var(--color-text-primary)] hover:no-underline"
				>
					Dashboard
				</a>
				<a
					href="/search"
					class="text-sm font-medium text-[var(--color-text-secondary)] no-underline transition-colors hover:text-[var(--color-text-primary)] hover:no-underline"
				>
					Search
				</a>
			</div>
		</div>
		<div class="flex items-center gap-4">
			{#if authError}
				<span class="text-sm text-[var(--color-accent-red)]">{authError}</span>
			{:else if currentUser}
				<span class="text-sm text-[var(--color-text-secondary)]">
					{currentUser.character_name}
				</span>
				<button
					onclick={logout}
					class="cursor-pointer rounded border border-[var(--color-border)] bg-transparent px-3 py-1 text-sm text-[var(--color-text-secondary)] transition-colors hover:border-[var(--color-accent-red)] hover:text-[var(--color-accent-red)]"
				>
					Logout
				</button>
			{:else}
				<a
					href="/api/auth/login"
					class="rounded border border-[var(--color-border)] px-3 py-1 text-sm text-[var(--color-accent-blue)] no-underline transition-colors hover:border-[var(--color-accent-blue)] hover:bg-[var(--color-accent-blue)] hover:text-white hover:no-underline"
				>
					Login with EVE
				</a>
			{/if}
		</div>
	</div>
</nav>

<main class="mx-auto max-w-[1280px] px-6 py-8">
	{@render children()}
</main>
