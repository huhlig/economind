<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { backtest, strategy } from '$lib/api.js';
  import type { BacktestRun, BacktestDetail, StrategyConfig } from '$lib/api.js';
  import { createChart, ColorType, AreaSeries } from 'lightweight-charts';

  let strategies = $state<StrategyConfig[]>([]);
  let runs = $state<BacktestRun[]>([]);
  let selectedRun = $state<BacktestDetail | null>(null);
  let loading = $state(true);
  let detailLoading = $state(false);
  let error = $state<string | null>(null);
  let submitting = $state(false);

  // New backtest form
  let formStrategyId = $state('');
  let formFrom = $state('2023-01-01');
  let formTo = $state('2024-01-01');
  let formCapital = $state('100000');

  let chartEl = $state<HTMLDivElement | undefined>(undefined);
  let chartInstance: ReturnType<typeof createChart> | null = null;

  onMount(async () => {
    try {
      [strategies, runs] = await Promise.all([strategy.list(), backtest.list()]);
      if (strategies.length > 0) formStrategyId = strategies[0].id;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  async function selectRun(run: BacktestRun) {
    detailLoading = true;
    try {
      selectedRun = await backtest.get(run.id);
      await tick();
      renderChart();
    } catch (e) {
      error = String(e);
    } finally {
      detailLoading = false;
    }
  }

  function renderChart() {
    if (!chartEl || !selectedRun) return;
    chartInstance?.remove();
    chartInstance = createChart(chartEl, {
      layout: {
        background: { type: ColorType.Solid, color: '#1e2130' },
        textColor: '#94a3b8',
      },
      grid: {
        vertLines: { color: '#2a2d3e' },
        horzLines: { color: '#2a2d3e' },
      },
      width: chartEl.clientWidth,
      height: 260,
    });
    const series = chartInstance.addSeries(AreaSeries, {
      lineColor: '#3b82f6',
      topColor: 'rgba(59,130,246,0.3)',
      bottomColor: 'rgba(59,130,246,0.0)',
    });
    const points = selectedRun.equity_curve.map(p => ({
      time: p.date as `${number}-${number}-${number}`,
      value: p.equity,
    }));
    series.setData(points);
    chartInstance.timeScale().fitContent();
  }

  function validateStrategy(id: string): string | null {
    const cfg = strategies.find(s => s.id === id);
    if (!cfg) return 'Strategy not found.';
    const roles = cfg.plugins.map(p => p.role.toLowerCase());
    const missing: string[] = [];
    if (!roles.includes('identifier')) missing.push('Identifier');
    if (!roles.includes('timer')) missing.push('Timer');
    if (!roles.includes('sizer')) missing.push('Sizer');
    if (missing.length) {
      return `Strategy is missing required plugins: ${missing.join(', ')}. Add them in the strategy editor before running a backtest.`;
    }
    return null;
  }

  async function submitBacktest() {
    if (!formStrategyId) return;
    const validationError = validateStrategy(formStrategyId);
    if (validationError) {
      error = validationError;
      return;
    }
    submitting = true;
    error = null;
    try {
      const run = await backtest.run({
        config_id: formStrategyId,
        from_date: formFrom,
        to_date: formTo,
        initial_capital: parseFloat(formCapital),
      });
      runs = [run, ...runs];
      await selectRun(run);
    } catch (e) {
      const msg = String(e);
      // Parse JSON error body if present
      try {
        const parsed = JSON.parse(msg.includes('{') ? msg.slice(msg.indexOf('{')) : '{}');
        error = parsed.error ?? msg;
      } catch {
        error = msg;
      }
    } finally {
      submitting = false;
    }
  }

  function fmt(n: number, d = 2) {
    return n.toLocaleString('en-US', { minimumFractionDigits: d, maximumFractionDigits: d });
  }

  function fmtPct(n: number) {
    return (n >= 0 ? '+' : '') + fmt(n * 100) + '%';
  }

  function pnlColor(n: number) {
    return n >= 0 ? 'var(--color-accent-green)' : 'var(--color-accent-red)';
  }
</script>

<div class="p-6">
  <h1 class="text-xl font-semibold mb-6" style="color: var(--color-text-primary)">Backtest</h1>

  {#if loading}
    <div style="color: var(--color-text-muted)">Loading…</div>
  {/if}
  {#if error}
    <div class="text-sm mb-4 rounded-lg px-3 py-2" style="color: var(--color-accent-red); background: rgba(239,68,68,0.1); border: 1px solid rgba(239,68,68,0.3);">
      {error}
      <button onclick={() => (error = null)} class="ml-2 opacity-60 hover:opacity-100">✕</button>
    </div>
  {/if}

  <div class="grid grid-cols-3 gap-6">
    <!-- Left panel: run list + new backtest form -->
    <div class="space-y-4">
      <!-- New backtest -->
      <div class="rounded-xl p-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
        <h2 class="text-sm font-medium mb-3" style="color: var(--color-text-primary)">New Backtest</h2>
        <div class="space-y-3">
          <div>
            <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Strategy</label>
            <select
              bind:value={formStrategyId}
              class="w-full rounded-lg px-3 py-1.5 text-sm"
              style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;"
            >
              {#each strategies as s}
                <option value={s.id}>{s.name}</option>
              {/each}
            </select>
          </div>
          <div class="grid grid-cols-2 gap-2">
            <div>
              <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">From</label>
              <input type="date" bind:value={formFrom} class="w-full rounded-lg px-2 py-1.5 text-xs"
                style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
            </div>
            <div>
              <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">To</label>
              <input type="date" bind:value={formTo} class="w-full rounded-lg px-2 py-1.5 text-xs"
                style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
            </div>
          </div>
          <div>
            <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Initial Capital ($)</label>
            <input type="number" bind:value={formCapital} class="w-full rounded-lg px-3 py-1.5 text-sm"
              style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
          </div>
          <button
            onclick={submitBacktest}
            disabled={submitting || !formStrategyId}
            class="w-full py-2 rounded-lg text-sm font-medium"
            style="background: var(--color-accent-blue); color: white; opacity: {submitting ? 0.6 : 1};"
          >
            {submitting ? 'Running…' : 'Run Backtest'}
          </button>
        </div>
      </div>

      <!-- Run history -->
      <div class="rounded-xl overflow-hidden" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
        <div class="px-4 py-3" style="border-bottom: 1px solid var(--color-border)">
          <span class="text-sm font-medium" style="color: var(--color-text-primary)">Run History</span>
        </div>
        <div class="divide-y" style="--tw-divide-opacity: 1; border-color: var(--color-border)">
          {#each runs as run (run.id)}
            <button
              onclick={() => selectRun(run)}
              class="w-full text-left px-4 py-3 transition-colors"
              style="background: {selectedRun?.run.id === run.id ? 'var(--color-bg-secondary)' : 'transparent'}"
            >
              <div class="text-xs font-mono" style="color: var(--color-text-muted)">{run.id.slice(0, 8)}…</div>
              <div class="text-xs mt-0.5 flex gap-3">
                <span style="color: {pnlColor(run.total_return ?? 0)}">
                  {run.total_return != null ? fmtPct(run.total_return) : '—'}
                </span>
                <span style="color: var(--color-text-muted)">{run.from_date} → {run.to_date}</span>
              </div>
            </button>
          {:else}
            <p class="px-4 py-4 text-xs text-center" style="color: var(--color-text-muted)">No runs yet.</p>
          {/each}
        </div>
      </div>
    </div>

    <!-- Right panel: detail -->
    <div class="col-span-2">
      {#if detailLoading}
        <div style="color: var(--color-text-muted)">Loading detail…</div>
      {:else if selectedRun}
        {@const r = selectedRun.run}
        <!-- Metrics -->
        <div class="grid grid-cols-4 gap-3 mb-4">
          {#each [
            { label: 'Total Return', value: r.total_return != null ? fmtPct(r.total_return) : '—', color: pnlColor(r.total_return ?? 0) },
            { label: 'Ann. Return', value: r.annualized_return != null ? fmtPct(r.annualized_return) : '—', color: pnlColor(r.annualized_return ?? 0) },
            { label: 'Sharpe', value: r.sharpe_ratio != null ? fmt(r.sharpe_ratio) : '—' },
            { label: 'Max DD', value: r.max_drawdown != null ? fmtPct(-r.max_drawdown) : '—', color: 'var(--color-accent-red)' },
            { label: 'Win Rate', value: r.win_rate != null ? fmtPct(r.win_rate) : '—' },
            { label: 'Total Trades', value: String(r.total_trades ?? 0) },
            { label: 'Initial Capital', value: '$' + fmt(r.initial_capital) },
            { label: 'Status', value: r.status },
          ] as m}
            <div class="rounded-lg p-3" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
              <div class="text-xs mb-1" style="color: var(--color-text-muted)">{m.label}</div>
              <div class="text-sm font-semibold" style="color: {m.color ?? 'var(--color-text-primary)'}">{m.value}</div>
            </div>
          {/each}
        </div>

        <!-- Equity curve -->
        <div class="rounded-xl p-4 mb-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
          <div class="text-sm font-medium mb-3" style="color: var(--color-text-primary)">Equity Curve</div>
          <div bind:this={chartEl}></div>
        </div>

        <!-- Trades table -->
        <div class="rounded-xl overflow-hidden" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
          <div class="px-4 py-3" style="border-bottom: 1px solid var(--color-border)">
            <span class="text-sm font-medium" style="color: var(--color-text-primary)">Trades ({selectedRun.trades.length})</span>
          </div>
          <div class="overflow-x-auto" style="max-height: 280px; overflow-y: auto;">
            <table class="w-full text-xs">
              <thead style="position: sticky; top: 0; background: var(--color-bg-card);">
                <tr style="border-bottom: 1px solid var(--color-border)">
                  <th class="text-left px-4 py-2 font-medium" style="color: var(--color-text-muted)">Symbol</th>
                  <th class="text-left px-4 py-2 font-medium" style="color: var(--color-text-muted)">Side</th>
                  <th class="text-right px-4 py-2 font-medium" style="color: var(--color-text-muted)">Entry</th>
                  <th class="text-right px-4 py-2 font-medium" style="color: var(--color-text-muted)">Exit</th>
                  <th class="text-right px-4 py-2 font-medium" style="color: var(--color-text-muted)">Qty</th>
                  <th class="text-right px-4 py-2 font-medium" style="color: var(--color-text-muted)">Net P&L</th>
                </tr>
              </thead>
              <tbody>
                {#each selectedRun.trades as t (t.id)}
                  <tr style="border-top: 1px solid var(--color-border)">
                    <td class="px-4 py-2 font-medium" style="color: var(--color-text-primary)">{t.symbol}</td>
                    <td class="px-4 py-2" style="color: {t.side === 'Long' ? 'var(--color-accent-green)' : 'var(--color-accent-red)'}">{t.side}</td>
                    <td class="px-4 py-2 text-right" style="color: var(--color-text-secondary)">${fmt(t.entry_price)}</td>
                    <td class="px-4 py-2 text-right" style="color: var(--color-text-secondary)">{t.exit_price != null ? '$' + fmt(t.exit_price) : '—'}</td>
                    <td class="px-4 py-2 text-right" style="color: var(--color-text-secondary)">{t.quantity}</td>
                    <td class="px-4 py-2 text-right font-medium" style="color: {pnlColor(t.net_pnl ?? 0)}">
                      {t.net_pnl != null ? (t.net_pnl >= 0 ? '+$' : '-$') + fmt(Math.abs(t.net_pnl)) : '—'}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </div>
      {:else}
        <div class="flex items-center justify-center h-64 rounded-xl" style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-muted)">
          Select a run from the history or run a new backtest.
        </div>
      {/if}
    </div>
  </div>
</div>
