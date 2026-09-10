<style>
	div, .a > p:hover { color: red; margin: 0 }
	@media (min-width: 10px) { :global(.b) { display: none } }
	.c { .d { x: y } }
</style>
<div class="a"><p>x</p></div>
