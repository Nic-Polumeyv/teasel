<script lang="ts">
	import { fade } from "svelte/transition";
	import { flip } from "svelte/animate";
	let items: number[] = [];
</script>

{#each items as item (item)}
	<li transition:fade={{ duration: 100 }} animate:flip={{ delay: 50 }}>{item}</li>
{/each}

{#snippet cell<T>(value: T, width = 1)}
	<td colspan={width}>{value}</td>
{/snippet}
{@render cell(1)}
