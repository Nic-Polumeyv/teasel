<script>
	let n = 1, a, b;
</script>

<svelte:element this="h{n}" />
<svelte:element this="{a}{/* c */ b}">x</svelte:element>
<svelte:element this='&amp;{n}' class="k" />
<svelte:element this=h{n} />
