<script lang="ts">
	interface I { x: number }
	let v: I = { x: 1 } as I;
	export let p: string;
</script>
{v.x satisfies number}
