<script lang="ts">
	let { data } = $props();
</script>

<svelte:head>
	<title>Page not found · teasel</title>
	<meta name="robots" content="noindex" />
</svelte:head>

<div class="mx-auto w-full max-w-6xl px-8 py-16 md:py-24">
	<h1 class="font-serif text-5xl font-medium tracking-tight">Page not found</h1>
	<div class="mt-10 grid gap-x-12 gap-y-8 md:grid-cols-3">
		{#each data.sections as section (section.label)}
			<div>
				<h2 class="label text-sm">{section.label}</h2>
				<ul class="mt-2">
					{#each section.pages as link (link.href)}
						<li><a href={link.href} class="text-primary underline decoration-primary/40 underline-offset-4 hover:decoration-primary">{link.title}</a></li>
					{/each}
				</ul>
			</div>
		{/each}
	</div>
</div>
