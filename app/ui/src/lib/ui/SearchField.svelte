<script lang="ts" generics="S extends string">
  import Icon from './Icon.svelte';

  let {
    value = $bindable(''),
    placeholder = 'Search',
    label = 'Search',
    scope = $bindable(),
    scopes,
    onsubmit,
    onkeydown,
    input = $bindable(),
  }: {
    value?: string;
    placeholder?: string;
    label?: string;
    /** The search scope, shown as a pop-up at the end ("Titles ⌄"). */
    scope?: S;
    scopes?: { value: S; label: string }[];
    onsubmit?: (value: string) => void;
    onkeydown?: (e: KeyboardEvent) => void;
    input?: HTMLInputElement;
  } = $props();
</script>

<label class="fo-search">
  <Icon name="search" size={14} weight={1.5} />
  <input
    bind:this={input}
    type="search"
    aria-label={label}
    {placeholder}
    bind:value
    onkeydown={(e) => {
      onkeydown?.(e);
      if (e.key === 'Enter') onsubmit?.(value);
      if (e.key === 'Escape' && value) {
        value = '';
        e.stopPropagation();
      }
    }}
  />
  {#if scopes?.length}
    <span class="scope">
      <select aria-label="Search in" bind:value={scope}>
        {#each scopes as s (s.value)}<option value={s.value}>{s.label}</option>{/each}
      </select>
      <span aria-hidden="true">{scopes.find((s) => s.value === scope)?.label ?? ''} ⌄</span>
    </span>
  {/if}
</label>

<style>
  .fo-search {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    box-sizing: border-box;
    padding: 0 8px;
    border-radius: var(--fo-radius-field);
    background: var(--fo-fill);
    color: var(--fo-label-2);
  }
  .fo-search:focus-within { box-shadow: 0 0 0 2px var(--fo-accent); }
  input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--fo-label);
    font: var(--fo-text-body);
    padding: 0;
  }
  input::placeholder { color: var(--fo-label-2); }
  input::-webkit-search-cancel-button { -webkit-appearance: none; }
  .scope { position: relative; font: var(--fo-text-caption); padding: 2px 6px; border-radius: 5px; background: var(--fo-fill); color: var(--fo-label-2); white-space: nowrap; }
  .scope select { position: absolute; inset: 0; opacity: 0; font: inherit; }
</style>
