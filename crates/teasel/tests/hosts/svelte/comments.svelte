<!-- a comment -->
<div /* in tag */ a={/* before */ x /* after */} {...r} /* end */>
	{/* leading */ y // trailing
	}
</div>

<script>
// leads the declaration
let items = []; // trails it
</script>

<div
  // stays in the list alone
  class="x" // so does this
  onclick={handler /* trails the handler */}
></div>

{#each items
  // trails the list, over the newline
  as item}{item}{/each}

{#snippet row(/* leads the parameter */ a, /* trails a */ b)}{a}{/snippet}

{#if ok /* trails ok */}{/if}
