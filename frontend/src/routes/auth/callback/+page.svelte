<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';

	let message = $state('Authenticating...');

	onMount(() => {
		const errorParam = $page.url.searchParams.get('error');
		const errorDesc = $page.url.searchParams.get('error_description');
		if (errorParam) {
			console.error('[nea] auth callback error', { error: errorParam, description: errorDesc });
			message = `Authentication failed: ${errorDesc || errorParam}`;
			return;
		}

		// The API handles the callback and redirects to /.
		// If we land here, wait briefly then redirect to home.
		setTimeout(() => {
			message = 'Redirecting...';
			goto('/');
		}, 2000);
	});
</script>

<div class="flex min-h-[60vh] items-center justify-center">
	<div class="text-center">
		{#if message.startsWith('Authentication failed')}
			<p class="text-lg text-[var(--color-accent-red)]">{message}</p>
			<a href="/" class="mt-4 inline-block text-sm">Back to Dashboard</a>
		{:else}
			<div class="mx-auto mb-4 h-8 w-8 animate-spin rounded-full border-2 border-[var(--color-border)] border-t-[var(--color-accent-blue)]"></div>
			<p class="text-lg text-[var(--color-text-secondary)]">{message}</p>
		{/if}
	</div>
</div>
