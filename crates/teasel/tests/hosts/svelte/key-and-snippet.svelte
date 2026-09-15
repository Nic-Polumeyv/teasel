{#key k}<Comp />{/key}
{#snippet row(item, i = 0)}
	<li>{i}: {item}</li>
{/snippet}
{@render row(x, 1)}
{@render row?.(y)}
