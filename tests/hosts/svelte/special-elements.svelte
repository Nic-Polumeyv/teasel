<svelte:options runes />
<svelte:head><title>{t}</title></svelte:head>
<svelte:window on:resize={r} />
<svelte:document />
<svelte:body />
<svelte:element this="h1" />
<svelte:element this={tag}>x</svelte:element>
<svelte:component this={C} prop={1} />
<svelte:self depth={d} />
<svelte:fragment slot="named">y</svelte:fragment>
<svelte:boundary><Inner /></svelte:boundary>
<slot name="s">fallback</slot>
