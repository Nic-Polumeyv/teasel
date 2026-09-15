{#each items as item, i (item.id)}
	{i}: {item}
{:else}
	none
{/each}
{#each list as { a, b = 2, ...rest }}{a}{/each}
{#each xs}{/each}
