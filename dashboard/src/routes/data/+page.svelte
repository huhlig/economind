<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { data, instruments } from '$lib/api.js';
  import type { Instrument, Bar } from '$lib/api.js';
  import { createChart, ColorType, CandlestickSeries } from 'lightweight-charts';

  let allInstruments = $state<Instrument[]>([]);
  let bars = $state<Bar[]>([]);
  let loading = $state(true);
  let barsLoading = $state(false);
  let error = $state<string | null>(null);

  let selectedSymbol = $state('');
  let fromDate = $state('2023-01-01');
  let toDate = $state('2024-01-01');

  let chartEl = $state<HTMLDivElement | undefined>(undefined);
  let chartInstance: ReturnType<typeof createChart> | null = null;

  onMount(async () => {
    try {
      allInstruments = await instruments.list();
      if (allInstruments.length > 0) selectedSymbol = allInstruments[0].symbol;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  async function loadBars() {
    if (!selectedSymbol) return;
    barsLoading = true;
    error = null;
    try {
      bars = await data.bars(selectedSymbol, fromDate, toDate);
      await tick();
      renderChart();
    } catch (e) {
      error = String(e);
    } finally {
      barsLoading = false;
    }
  }

  function renderChart() {
    if (!chartEl || bars.length === 0) return;
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
      height: 360,
    });

    const candleSeries = chartInstance.addSeries(CandlestickSeries, {
      upColor: '#22c55e',
      downColor: '#ef4444',
      borderVisible: false,
      wickUpColor: '#22c55e',
      wickDownColor: '#ef4444',
    });

    candleSeries.setData(
      bars.map(b => ({
        time: b.date as `${number}-${number}-${number}`,
        open: b.open,
        high: b.high,
        low: b.low,
        close: b.close,
      }))
    );
    chartInstance.timeScale().fitContent();
  }

  function fmt(n: number, d = 2) {
    return n.toLocaleString('en-US', { minimumFractionDigits: d, maximumFractionDigits: d });
  }
</script>

<div class="p-6">
  <h1 class="text-xl font-semibold mb-6" style="color: var(--color-text-primary)">Data Explorer</h1>

  <!-- Controls -->
  <div class="flex gap-3 mb-5 flex-wrap">
    {#if loading}
      <span class="text-sm" style="color: var(--color-text-muted)">Loading instruments…</span>
    {:else}
      <select
        bind:value={selectedSymbol}
        class="rounded-lg px-3 py-1.5 text-sm"
        style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;"
      >
        {#each allInstruments as inst}
          <option value={inst.symbol}>{inst.symbol} — {inst.name}</option>
        {/each}
      </select>

      <input type="date" bind:value={fromDate} class="rounded-lg px-3 py-1.5 text-sm"
        style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
      <input type="date" bind:value={toDate} class="rounded-lg px-3 py-1.5 text-sm"
        style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />

      <button
        onclick={loadBars}
        disabled={barsLoading || !selectedSymbol}
        class="px-4 py-1.5 rounded-lg text-sm font-medium"
        style="background: var(--color-accent-blue); color: white; opacity: {barsLoading ? 0.6 : 1};"
      >
        {barsLoading ? 'Loading…' : 'Load Bars'}
      </button>
    {/if}
  </div>

  {#if error}
    <div class="text-sm mb-4" style="color: var(--color-accent-red)">{error}</div>
  {/if}

  {#if bars.length > 0}
    <!-- Chart -->
    <div class="rounded-xl p-4 mb-5" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
      <div class="text-sm font-medium mb-3" style="color: var(--color-text-primary)">
        {selectedSymbol} · {bars.length} bars
      </div>
      <div bind:this={chartEl}></div>
    </div>

    <!-- OHLCV table -->
    <div class="rounded-xl overflow-hidden" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
      <div class="overflow-x-auto" style="max-height: 300px; overflow-y: auto;">
        <table class="w-full text-xs">
          <thead style="position: sticky; top: 0; background: var(--color-bg-card)">
            <tr style="border-bottom: 1px solid var(--color-border)">
              <th class="text-left px-4 py-2 font-medium" style="color: var(--color-text-muted)">Date</th>
              <th class="text-right px-4 py-2 font-medium" style="color: var(--color-text-muted)">Open</th>
              <th class="text-right px-4 py-2 font-medium" style="color: var(--color-text-muted)">High</th>
              <th class="text-right px-4 py-2 font-medium" style="color: var(--color-text-muted)">Low</th>
              <th class="text-right px-4 py-2 font-medium" style="color: var(--color-text-muted)">Close</th>
              <th class="text-right px-4 py-2 font-medium" style="color: var(--color-text-muted)">Volume</th>
            </tr>
          </thead>
          <tbody>
            {#each bars.slice().reverse() as b}
              <tr style="border-top: 1px solid var(--color-border)">
                <td class="px-4 py-1.5" style="color: var(--color-text-secondary)">{b.date}</td>
                <td class="px-4 py-1.5 text-right" style="color: var(--color-text-secondary)">{fmt(b.open)}</td>
                <td class="px-4 py-1.5 text-right" style="color: var(--color-accent-green)">{fmt(b.high)}</td>
                <td class="px-4 py-1.5 text-right" style="color: var(--color-accent-red)">{fmt(b.low)}</td>
                <td class="px-4 py-1.5 text-right font-medium" style="color: {b.close >= b.open ? 'var(--color-accent-green)' : 'var(--color-accent-red)'}">{fmt(b.close)}</td>
                <td class="px-4 py-1.5 text-right" style="color: var(--color-text-muted)">{b.volume.toLocaleString()}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>
  {:else if !barsLoading}
    <div class="flex items-center justify-center h-48 rounded-xl" style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-muted)">
      Select a symbol and date range, then click Load Bars.
    </div>
  {/if}
</div>
