{#each a, b as x}{x}{/each}
{#each items, i}{i}{/each}
{#each f(a, b), i}{i}{/each}
{#each a, b as x, i (x.id)}{x}{i}{/each}
{#each log(items), items as { id }}{id}{/each}
{#each [a, b] as x}{x}{/each}
