<script lang="ts">
	import type { Snippet } from 'svelte';
	import { center, type Rect } from './geometry.ts';

	const tints = {
		sky: 'fill-sky-400/15 stroke-sky-400/60',
		amber: 'fill-amber-400/15 stroke-amber-400/60',
		violet: 'fill-violet-400/15 stroke-violet-400/60',
		emerald: 'fill-emerald-400/15 stroke-emerald-400/60',
		rose: 'fill-rose-400/15 stroke-rose-400/60',
	};

	let { rect, tint, dashed = false, size, children }: { rect: Rect; tint?: keyof typeof tints; dashed?: boolean; size?: number; children?: Snippet } = $props();

	const mid = $derived(center(rect));
</script>

<rect x={rect.x} y={rect.y} width={rect.w} height={rect.h} rx={Math.min(8, rect.h / 4)} fill="none" stroke="currentColor" stroke-width="1.5" stroke-dasharray={dashed ? '3 3' : undefined} class={tint && tints[tint]} />
{#if children}
	<text x={mid.x} y={mid.y} text-anchor="middle" dominant-baseline="central" font-size={size}>{@render children()}</text>
{/if}
