<script lang="ts">
  import { onMount } from 'svelte';
  import { signals, strategy } from '$lib/api.js';
  import type { Signal, StrategyConfig } from '$lib/api.js';
  import { eventLog } from '$lib/stores/events.js';
  import { pageContext } from '$lib/stores/pageContext.js';

  let allSignals = $state<Signal[]>([]);
  let strategies = $state<StrategyConfig[]>([]);
  let loading = $state(true);
  let searching = $state(false);
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

  $effect(() => {
    pageContext.set(`[Signals Explorer]\n- Loaded ${allSignals.length} signals\n- ${strategies.length} strategies available`);
    return () => pageContext.set('');
  });

  // Refresh signals when a new signal event arrives
  $effect(() => {
    const latest = $eventLog[0];
    if (latest?.event.type === 'signal_emitted') {
      search();
    }
  });

  async function search() {
    searching = true;
    error = null;
    try {
      allSignals = await signals.list({
        strategy_id: filterStrategy || undefined,
        symbol: filterSymbol.trim() || undefined,
        limit: 200,
      });
    } catch (e) {
      error = String(e);
    } finally {
      searching = false;
    }
  }

  // Client-side direction filter (not a server param)
  let filtered = $derived(
    filterDirection
      ? allSignals.filter(s => s.direction === filterDirection)
      : allSignals
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
  <div class="flex flex-wrap gap-3 mb-5 items-end">
    <div>
      <label class="block text-xs mb-1" style="color: var(--color-text-muted)">Strategy</label>
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
    </div>

    <div>
      <label class="block text-xs mb-1" style="color: var(--color-text-muted)">Symbol</label>
      <input
        type="text"
        placeholder="e.g. AAPL"
        bind:value={filterSymbol}
        onkeydown={(e) => e.key === 'Enter' && search()}
        class="rounded-lg px-3 py-1.5 text-sm w-28"
        style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;"
      />
    </div>

    <div>
      <label class="block text-xs mb-1" style="color: var(--color-text-muted)">Direction</label>
      <select
        bind:value={filterDirection}
        class="rounded-lg px-3 py-1.5 text-sm"
        style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-secondary); outline: none;"
      >
        <option value="">All</option>
        <option value="Long">Long</option>
        <option value="Short">Short</option>
        <option value="Flat">Flat</option>
      </select>
    </div>

    <button
      onclick={search}
      disabled={searching}
      class="px-4 py-1.5 rounded-lg text-sm font-medium"
      style="background: var(--color-accent-blue); color: #fff; opacity: {searching ? 0.6 : 1};"
    >
      {searching ? 'Searching…' : 'Search'}
    </button>

    <span class="ml-auto text-xs self-center" style="color: var(--color-text-muted)">
      {filtered.length} signal{filtered.length !== 1 ? 's' : ''}
    </span>
  </div>

  {#if loading}
    <div style="color: var(--color-text-muted)">Loading…</div>
  {:else if error}
    <div class="text-sm mb-4" style="color: var(--color-accent-red)">{error}</div>
  {:else}
    <div class="rounded-xl overflow-hidden" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
      <table class="w-full text-sm">
        <thead>
          <tr style="border-bottom: 1px solid var(--color-border);">
            <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Symbol</th>
            <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Direction</th>
            <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">ID Score</th>
            <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Timing Score</th>
            <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Shares</th>
            <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Strategy</th>
            <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Emitted At</th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as sig (sig.id)}
            <tr style="border-top: 1px solid var(--color-border)">
              <td class="px-4 py-2.5 font-medium" style="color: var(--color-text-primary)">{sig.symbol}</td>
              <td class="px-4 py-2.5 font-medium" style="color: {dirColor(sig.direction)}">{sig.direction}</td>
              <td class="px-4 py-2.5" style="color: var(--color-text-secondary)">
                {sig.strength != null ? Number(sig.strength).toFixed(3) : (sig as any).identifier_score ?? '—'}
              </td>
              <td class="px-4 py-2.5" style="color: var(--color-text-secondary)">
                {(sig as any).timing_score ?? '—'}
              </td>
              <td class="px-4 py-2.5" style="color: var(--color-text-muted)">
                {(sig as any).position_shares ?? '—'}
              </td>
              <td class="px-4 py-2.5 text-xs" style="color: var(--color-text-muted)">
                {strategies.find(s => s.id === sig.strategy_config_id)?.name ?? (sig.strategy_config_id?.slice(0, 8) ?? '—')}
              </td>
              <td class="px-4 py-2.5 text-xs" style="color: var(--color-text-muted)">
                {new Date(sig.generated_at || (sig as any).emitted_at).toLocaleString()}
              </td>
            </tr>
          {:else}
            <tr>
              <td colspan="7" class="px-4 py-8 text-center text-sm" style="color: var(--color-text-muted)">
                {#if filterStrategy || filterSymbol.trim()}
                  No signals found for these filters. Try different criteria or run a strategy first.
                {:else}
                  No signals yet. Run a strategy or backtest to generate signals.
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
