<script>
	let a = 1, b = 2;
	function f() { return a }
</script>
{#each xs as a}{a}{b}{/each}
<Comp let:a>{a}{f}</Comp>
{#snippet s(b)}{b}{a}{/snippet}
{a}
