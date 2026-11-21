<script module>
	// the module's own
	export const m = 1;
</script>

<!-- svelte-ignore a11y_missing_attribute -->
<style>
	p { color: red; }
</style>

<div a={(x) /* trails x past its paren */} b={/* the paren's */ (y)}>
	{f(/* read, attached nowhere */)}
</div>

{#each items as { a /* trails a */, b }, i (key)}{a}{/each}
{#snippet row(/* leads a */ a /* trails a */, b, /* trails b */)}{a}{/snippet}

<!-- leads the program, as a line comment -->
	<script>/* blanked
	   before the script */
	/**
	 * leads f, and the module's comments and the markup's before it
	 */
	function f() {
		/* nothing to lead,
		   so it trails */
	}
</script>
