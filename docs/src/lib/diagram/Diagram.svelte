<script lang="ts">
	import type { Snippet } from 'svelte';

	let { width, height, label, children }: { width: number; height: number; label: string; children: Snippet } = $props();
</script>

<svg viewBox="0 0 {width} {height}" width="100%" role="img" aria-label={label} fill="currentColor" font-size="14" class="my-8">
	{@render children()}
</svg>
