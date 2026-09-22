<script>let x = 1;</script>
<Comp let:x p={x}>
  {x}
  <div slot="first">{x}</div>
  <div slot="second" let:y>{x}{y}</div>
</Comp>
