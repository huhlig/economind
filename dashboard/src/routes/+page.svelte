<script lang="ts">
  import { onMount } from 'svelte';
  import { portfolio, signals, strategy, backtest } from '$lib/api.js';
  import type { PortfolioSummary, Signal, StrategyConfig, BacktestRun } from '$lib/api.js';
  import { eventLog } from '$lib/stores/events.js';

  let portfolioData = $state<PortfolioSummary | null>(null);
  let recentSignals = $state<Signal[]>([]);
  let strategies = $state<StrategyConfig[]>([]);
  let recentBacktests = $state<BacktestRun[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      [portfolioData, recentSignals, strategies, recentBacktests] = await Promise.all([
        portfolio.summary(),
        signals.list({ limit: 10 }),
        strategy.list(),
        backtest.list(),
      ]);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  $effect(() => {
    const latest = $eventLog[0];
    if (latest?.event.type === 'signal_emitted' || latest?.event.type === 'position_opened' || latest?.event.type === 'position_closed') {
      Promise.all([portfolio.summary(), signals.list({ limit: 10 })]).then(([p, s]) => {
        portfolioData = p;
        recentSignals = s;
      }).catch(() => {});
    }
  });

  function fmt(n: number, decimals = 2) {
    return n.toLocaleString('en-US', { minimumFractionDigits: decimals, maximumFractionDigits: decimals });
  }

  function fmtPct(n: number) {
    const s = fmt(Math.abs(n) * 100);
    return (n >= 0 ? '+' : '−') + s + '%';
  }

  function pnlColor(n: number) {
    return n >= 0 ? 'var(--color-accent-green)' : 'var(--color-accent-red)';
  }
</script>

<div class="p-6">
  <h1 class="text-xl font-semibold mb-6" style="color: var(--color-text-primary)">Overview</h1>

  {#if loading}
    <div style="color: var(--color-text-muted)">Loading…</div>
  {:else if error}
    <div class="text-sm" style="color: var(--color-accent-red)">{error}</div>
  {:else}
    <!-- KPI Cards -->
    <div class="grid grid-cols-4 gap-4 mb-8">
      {#each [
        { label: 'Total Equity', value: portfolioData ? '$' + fmt(portfolioData.total_equity) : '—' },
        { label: 'Cash', value: portfolioData ? '$' + fmt(portfolioData.cash) : '—' },
        { label: 'Open Positions', value: String(portfolioData?.positions.length ?? 0) },
        { label: 'Active Strategies', value: String(strategies.filter(s => s.enabled).length) },
      ] as card}
        <div class="rounded-xl p-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
          <div class="text-xs mb-1" style="color: var(--color-text-muted)">{card.label}</div>
          <div class="text-xl font-semibold" style="color: var(--color-text-primary)">{card.value}</div>
        </div>
      {/each}
    </div>

    <div class="grid grid-cols-2 gap-6">
      <!-- Recent Signals -->
      <div class="rounded-xl p-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
        <div class="flex items-center justify-between mb-3">
          <span class="text-sm font-medium" style="color: var(--color-text-primary)">Recent Signals</span>
          <a href="/signals" class="text-xs" style="color: var(--color-accent-blue)">View all →</a>
        </div>
        {#if recentSignals.length === 0}
          <p class="text-xs" style="color: var(--color-text-muted)">No signals yet.</p>
        {:else}
          <table class="w-full text-xs">
            <thead>
              <tr style="color: var(--color-text-muted)">
                <th class="text-left pb-2">Symbol</th>
                <th class="text-left pb-2">Type</th>
                <th class="text-left pb-2">Direction</th>
                <th class="text-left pb-2">Time</th>
              </tr>
            </thead>
            <tbody>
              {#each recentSignals as sig}
                <tr style="border-top: 1px solid var(--color-border)">
                  <td class="py-1.5 font-medium" style="color: var(--color-text-primary)">{sig.symbol}</td>
                  <td class="py-1.5" style="color: var(--color-text-secondary)">{sig.signal_type}</td>
                  <td class="py-1.5 font-medium" style="color: {sig.direction === 'Long' ? 'var(--color-accent-green)' : sig.direction === 'Short' ? 'var(--color-accent-red)' : 'var(--color-text-muted)'}">
                    {sig.direction}
                  </td>
                  <td class="py-1.5" style="color: var(--color-text-muted)">{new Date(sig.generated_at).toLocaleTimeString()}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>

      <!-- Live Event Log -->
      <div class="rounded-xl p-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
        <div class="flex items-center justify-between mb-3">
          <span class="text-sm font-medium" style="color: var(--color-text-primary)">Live Events</span>
          <button onclick={() => eventLog.clear()} class="text-xs" style="color: var(--color-text-muted)">Clear</button>
        </div>
        <div class="space-y-1 overflow-y-auto" style="max-height: 240px;">
          {#each $eventLog.slice(0, 20) as entry (entry.id)}
            <div class="text-xs rounded px-2 py-1" style="background: var(--color-bg-secondary)">
              <span style="color: var(--color-text-muted)">{entry.ts.toLocaleTimeString()} </span>
              <span style="color: var(--color-accent-blue)">{entry.event.type}</span>
            </div>
          {:else}
            <p class="text-xs" style="color: var(--color-text-muted)">Waiting for events…</p>
          {/each}
        </div>
      </div>

      <!-- Open Positions -->
      <div class="rounded-xl p-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
        <div class="flex items-center justify-between mb-3">
          <span class="text-sm font-medium" style="color: var(--color-text-primary)">Open Positions</span>
          <a href="/portfolio" class="text-xs" style="color: var(--color-accent-blue)">View all →</a>
        </div>
        {#if !portfolioData || portfolioData.positions.length === 0}
          <p class="text-xs" style="color: var(--color-text-muted)">No open positions.</p>
        {:else}
          <table class="w-full text-xs">
            <thead>
              <tr style="color: var(--color-text-muted)">
                <th class="text-left pb-2">Symbol</th>
                <th class="text-right pb-2">Qty</th>
                <th class="text-right pb-2">Avg Cost</th>
                <th class="text-right pb-2">Unrealized P&L</th>
              </tr>
            </thead>
            <tbody>
              {#each portfolioData.positions as pos}
                <tr style="border-top: 1px solid var(--color-border)">
                  <td class="py-1.5 font-medium" style="color: var(--color-text-primary)">{pos.symbol}</td>
                  <td class="py-1.5 text-right" style="color: var(--color-text-secondary)">{pos.quantity}</td>
                  <td class="py-1.5 text-right" style="color: var(--color-text-secondary)">${fmt(pos.average_cost)}</td>
                  <td class="py-1.5 text-right font-medium" style="color: {pnlColor(pos.unrealized_pnl ?? 0)}">
                    {pos.unrealized_pnl != null ? '$' + fmt(pos.unrealized_pnl) : '—'}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>

      <!-- Recent Backtests -->
      <div class="rounded-xl p-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
        <div class="flex items-center justify-between mb-3">
          <span class="text-sm font-medium" style="color: var(--color-text-primary)">Recent Backtests</span>
          <a href="/backtest" class="text-xs" style="color: var(--color-accent-blue)">View all →</a>
        </div>
        {#if recentBacktests.length === 0}
          <p class="text-xs" style="color: var(--color-text-muted)">No backtests run yet.</p>
        {:else}
          <table class="w-full text-xs">
            <thead>
              <tr style="color: var(--color-text-muted)">
                <th class="text-left pb-2">Run</th>
                <th class="text-right pb-2">Return</th>
                <th class="text-right pb-2">Sharpe</th>
                <th class="text-right pb-2">Max DD</th>
              </tr>
            </thead>
            <tbody>
              {#each recentBacktests.slice(0, 5) as run}
                <tr style="border-top: 1px solid var(--color-border)">
                  <td class="py-1.5" style="color: var(--color-text-secondary)">{run.id.slice(0, 8)}…</td>
                  <td class="py-1.5 text-right font-medium" style="color: {pnlColor(run.total_return ?? 0)}">
                    {run.total_return != null ? fmtPct(run.total_return) : '—'}
                  </td>
                  <td class="py-1.5 text-right" style="color: var(--color-text-secondary)">
                    {run.sharpe_ratio != null ? fmt(run.sharpe_ratio) : '—'}
                  </td>
                  <td class="py-1.5 text-right" style="color: var(--color-accent-red)">
                    {run.max_drawdown != null ? fmtPct(-run.max_drawdown) : '—'}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    </div>
  {/if}
</div>
