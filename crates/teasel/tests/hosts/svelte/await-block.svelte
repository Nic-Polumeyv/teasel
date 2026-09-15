{#await p}
	waiting
{:then value}
	{value}
{:catch error}
	{error.message}
{/await}
{#await p then v}{v}{/await}
{#await p catch e}{e}{/await}
