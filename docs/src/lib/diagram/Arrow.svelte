<script lang="ts">
	import { arrow, type Anchor, type Rect } from './geometry.ts';

	let { from, to, bend, class: klass }: { from: Rect | Anchor; to: Rect | Anchor; bend?: number; class?: string } = $props();

</script>

<path d={arrow(from, to, bend)} fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" stroke-linecap="round" class={klass} />
