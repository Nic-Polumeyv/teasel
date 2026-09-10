<script module>
	export const shared = 1;
</script>
<script lang="ts">
	let n: number = shared;
	export let name = 'x';
</script>
{n} {name}
