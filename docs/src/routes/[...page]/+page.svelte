<script lang="ts">
	let { data } = $props();

	function copy(event: MouseEvent) {
		const button = (event.target as HTMLElement).closest<HTMLButtonElement>('[data-copy]');
		if (!button) return;
		navigator.clipboard.writeText(button.dataset.copy!);
		button.textContent = 'Copied';
		setTimeout(() => (button.textContent = 'Copy'), 1500);
	}
</script>

<svelte:head>
	<title>{data.title} · teasel</title>
</svelte:head>

<h1>{data.title}</h1>
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div onclick={copy}>{@html data.html}</div>
