<script lang="ts">
  import { onMount } from 'svelte';
  import { portfolio } from '$lib/api.js';
  import type { PortfolioSummary } from '$lib/api.js';
  import { eventLog } from '$lib/stores/events.js';

  let data = $state<PortfolioSummary | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  async function load() {
    try {
      data = await portfolio.summary();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  onMount(load);

  // Refresh on position events
  $effect(() => {
    const latest = $eventLog[0];
    if (latest?.event.type === 'position_opened' || latest?.event.type === 'position_closed') {
      load();
    }
  });

  function fmt(n: number, d = 2) {
    return n.toLocaleString('en-US', { minimumFractionDigits: d, maximumFractionDigits: d });
  }

  function pnlColor(n: number) {
    return n >= 0 ? 'var(--color-accent-green)' : 'var(--color-accent-red)';
  }
</script>

<div class="p-6">
  <h1 class="text-xl font-semibold mb-6" style="color: var(--color-text-primary)">Portfolio</h1>

  {#if loading}
    <div style="color: var(--color-text-muted)">Loading…</div>
  {:else if error}
    <div class="text-sm" style="color: var(--color-accent-red)">{error}</div>
  {:else if data}
    <!-- Summary KPIs -->
    <div class="grid grid-cols-3 gap-4 mb-8">
      {#each [
        { label: 'Total Equity', value: '$' + fmt(data.total_equity) },
        { label: 'Cash', value: '$' + fmt(data.cash) },
        { label: 'Unrealized P&L', value: (data.unrealized_pnl >= 0 ? '+$' : '-$') + fmt(Math.abs(data.unrealized_pnl)), color: pnlColor(data.unrealized_pnl) },
      ] as card}
        <div class="rounded-xl p-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
          <div class="text-xs mb-1" style="color: var(--color-text-muted)">{card.label}</div>
          <div class="text-xl font-semibold" style="color: {card.color ?? 'var(--color-text-primary)'}">
            {card.value}
          </div>
        </div>
      {/each}
    </div>

    <!-- Positions Table -->
    <div class="rounded-xl overflow-hidden" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
      <div class="px-4 py-3" style="border-bottom: 1px solid var(--color-border)">
        <span class="text-sm font-medium" style="color: var(--color-text-primary)">Open Positions ({data.positions.length})</span>
      </div>
      {#if data.positions.length === 0}
        <p class="px-4 py-6 text-sm text-center" style="color: var(--color-text-muted)">No open positions.</p>
      {:else}
        <table class="w-full text-sm">
          <thead>
            <tr style="border-bottom: 1px solid var(--color-border)">
              <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Symbol</th>
              <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Side</th>
              <th class="text-right px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Quantity</th>
              <th class="text-right px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Avg Cost</th>
              <th class="text-right px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Current Price</th>
              <th class="text-right px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Unrealized P&L</th>
            </tr>
          </thead>
          <tbody>
            {#each data.positions as pos}
              <tr style="border-top: 1px solid var(--color-border)">
                <td class="px-4 py-2.5 font-medium" style="color: var(--color-text-primary)">{pos.symbol}</td>
                <td class="px-4 py-2.5 font-medium" style="color: {pos.side === 'Long' ? 'var(--color-accent-green)' : 'var(--color-accent-red)'}">
                  {pos.side}
                </td>
                <td class="px-4 py-2.5 text-right" style="color: var(--color-text-secondary)">{pos.quantity}</td>
                <td class="px-4 py-2.5 text-right" style="color: var(--color-text-secondary)">${fmt(pos.average_cost)}</td>
                <td class="px-4 py-2.5 text-right" style="color: var(--color-text-secondary)">
                  {pos.current_price != null ? '$' + fmt(pos.current_price) : '—'}
                </td>
                <td class="px-4 py-2.5 text-right font-medium" style="color: {pnlColor(pos.unrealized_pnl ?? 0)}">
                  {pos.unrealized_pnl != null
                    ? (pos.unrealized_pnl >= 0 ? '+$' : '-$') + fmt(Math.abs(pos.unrealized_pnl))
                    : '—'}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>
  {/if}
</div>
