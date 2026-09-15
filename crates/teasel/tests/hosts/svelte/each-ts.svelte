<script lang="ts">
	let xs: number[] = [];
	let ys: number[] = [];
	let c = true;
	type T = number;
</script>

{#each xs as T[] || [] as item}{item}{/each}
{#each xs as T[] + ys as item}{item}{/each}
{#each c ? xs as T[] : ys as item}{item}{/each}
{#each xs as T[] as item, i (item)}{item}{i}{/each}
{#each xs as item, i}{item}{i}{/each}
{#each (xs as T[]).map(Number) as item}{item}{/each}
