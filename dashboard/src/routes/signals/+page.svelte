<script lang="ts">
  import { onMount } from 'svelte';
  import { signals, strategy } from '$lib/api.js';
  import type { Signal, StrategyConfig } from '$lib/api.js';
  import { eventLog } from '$lib/stores/events.js';

  let allSignals = $state<Signal[]>([]);
  let strategies = $state<StrategyConfig[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  let filterStrategy = $state('');
  let filterSymbol = $state('');
  let filterDirection = $state('');

  onMount(async () => {
    try {
      [allSignals, strategies] = await Promise.all([signals.list({ limit: 200 }), strategy.list()]);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  // Refresh signals when a new signal event arrives
  $effect(() => {
    const latest = $eventLog[0];
    if (latest?.event.type === 'signal_emitted') {
      signals.list({ limit: 200 }).then((s) => (allSignals = s)).catch(() => {});
    }
  });

  let filtered = $derived(
    allSignals.filter((s) => {
      if (filterStrategy && s.strategy_config_id !== filterStrategy) return false;
      if (filterSymbol && !s.symbol.toLowerCase().includes(filterSymbol.toLowerCase())) return false;
      if (filterDirection && s.direction !== filterDirection) return false;
      return true;
    })
  );

  function dirColor(d: string) {
    if (d === 'Long') return 'var(--color-accent-green)';
    if (d === 'Short') return 'var(--color-accent-red)';
    return 'var(--color-text-muted)';
  }
</script>

<div class="p-6">
  <h1 class="text-xl font-semibold mb-6" style="color: var(--color-text-primary)">Signals Explorer</h1>

  <!-- Filters -->
  <div class="flex gap-3 mb-5">
    <select
      bind:value={filterStrategy}
      class="rounded-lg px-3 py-1.5 text-sm"
      style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-secondary); outline: none;"
    >
      <option value="">All Strategies</option>
      {#each strategies as s}
        <option value={s.id}>{s.name}</option>
      {/each}
    </select>

    <input
      type="text"
      placeholder="Symbol…"
      bind:value={filterSymbol}
      class="rounded-lg px-3 py-1.5 text-sm w-32"
      style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;"
    />

    <select
      bind:value={filterDirection}
      class="rounded-lg px-3 py-1.5 text-sm"
      style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-secondary); outline: none;"
    >
      <option value="">All Directions</option>
      <option value="Long">Long</option>
      <option value="Short">Short</option>
      <option value="Flat">Flat</option>
    </select>

    <span class="ml-auto text-xs self-center" style="color: var(--color-text-muted)">
      {filtered.length} signals
    </span>
  </div>

  {#if loading}
    <div style="color: var(--color-text-muted)">Loading…</div>
  {:else if error}
    <div class="text-sm" style="color: var(--color-accent-red)">{error}</div>
  {:else}
    <div class="rounded-xl overflow-hidden" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
      <table class="w-full text-sm">
        <thead>
          <tr style="border-bottom: 1px solid var(--color-border);">
            <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Symbol</th>
            <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Type</th>
            <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Direction</th>
            <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Strength</th>
            <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Strategy</th>
            <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Generated At</th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as sig (sig.id)}
            <tr style="border-top: 1px solid var(--color-border)">
              <td class="px-4 py-2.5 font-medium" style="color: var(--color-text-primary)">{sig.symbol}</td>
              <td class="px-4 py-2.5" style="color: var(--color-text-secondary)">{sig.signal_type}</td>
              <td class="px-4 py-2.5 font-medium" style="color: {dirColor(sig.direction)}">{sig.direction}</td>
              <td class="px-4 py-2.5" style="color: var(--color-text-secondary)">
                {sig.strength != null ? sig.strength.toFixed(3) : '—'}
              </td>
              <td class="px-4 py-2.5 text-xs" style="color: var(--color-text-muted)">
                {strategies.find(s => s.id === sig.strategy_config_id)?.name ?? sig.strategy_config_id.slice(0, 8)}
              </td>
              <td class="px-4 py-2.5 text-xs" style="color: var(--color-text-muted)">
                {new Date(sig.generated_at).toLocaleString()}
              </td>
            </tr>
          {:else}
            <tr>
              <td colspan="6" class="px-4 py-6 text-center text-sm" style="color: var(--color-text-muted)">
                No signals match the current filters.
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
