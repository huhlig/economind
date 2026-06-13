<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { data, instruments } from '$lib/api.js';
  import type { SymbolCoverage, MacroSeriesEntry, Bar } from '$lib/api.js';
  import { createChart, ColorType, CandlestickSeries } from 'lightweight-charts';
  import { pageContext } from '$lib/stores/pageContext.js';

  // ── Tab state ────────────────────────────────────────────────────────────────
  let activeTab = $state<'manager' | 'explorer'>('manager');

  // ── Manager state ────────────────────────────────────────────────────────────
  let symbols = $state<SymbolCoverage[]>([]);
  let macroSeries = $state<MacroSeriesEntry[]>([]);
  let catalogLoading = $state(true);
  let catalogError = $state<string | null>(null);

  // Fetch form
  let fetchService = $state<'bars' | 'macro' | 'fundamentals'>('bars');
  let fetchSymbols = $state('');
  let fetchSince = $state('');
  let fetching = $state(false);
  let fetchResult = $state<string | null>(null);
  let fetchError = $state<string | null>(null);

  // Inventory search
  let inventorySearch = $state('');

  // Add symbols
  let addSymbolsInput = $state('');
  let adding = $state(false);
  let addError = $state<string | null>(null);
  let addResult = $state<string | null>(null);
  let removing = $state<string | null>(null); // symbol being removed

  // ── Explorer state ────────────────────────────────────────────────────────────
  let explorerSymbol = $state('');
  let explorerFrom = $state('2023-01-01');
  let explorerTo = $state(new Date().toISOString().slice(0, 10));
  let bars = $state<Bar[]>([]);
  let barsLoading = $state(false);
  let barsError = $state<string | null>(null);

  let chartEl = $state<HTMLDivElement | undefined>(undefined);
  let chartInstance: ReturnType<typeof createChart> | null = null;

  // ── Load catalog on mount ─────────────────────────────────────────────────────
  onMount(async () => {
    try {
      const catalog = await data.catalog();
      symbols = catalog.symbols;
      macroSeries = catalog.macro_series;
    } catch (e) {
      catalogError = String(e);
    } finally {
      catalogLoading = false;
    }
  });

  $effect(() => {
    const withBars = symbols.filter(s => s.bar_count > 0).length;
    const withFundamentals = symbols.filter(s => s.income_count > 0).length;
    pageContext.set(
      `[Data Manager]\n- ${symbols.length} instruments tracked\n` +
      `- ${withBars} have daily bar history\n` +
      `- ${withFundamentals} have fundamentals\n` +
      `- ${macroSeries.length} macro series loaded`
    );
    return () => pageContext.set('');
  });

  // ── Inventory filter ─────────────────────────────────────────────────────────
  let filteredSymbols = $derived(
    inventorySearch.trim()
      ? symbols.filter(s =>
          s.symbol.toLowerCase().includes(inventorySearch.toLowerCase()) ||
          s.name.toLowerCase().includes(inventorySearch.toLowerCase())
        )
      : symbols
  );

  // ── Add / remove symbols ─────────────────────────────────────────────────────
  async function addSymbols() {
    const syms = addSymbolsInput.split(/[\s,]+/).map(s => s.trim().toUpperCase()).filter(Boolean);
    if (syms.length === 0) return;
    adding = true;
    addError = null;
    addResult = null;
    try {
      await Promise.all(syms.map(s => instruments.add(s)));
      addResult = `Added ${syms.length} symbol${syms.length > 1 ? 's' : ''}: ${syms.join(', ')}`;
      addSymbolsInput = '';
      // Refresh catalog
      const catalog = await data.catalog();
      symbols = catalog.symbols;
      macroSeries = catalog.macro_series;
    } catch (e) {
      addError = String(e);
    } finally {
      adding = false;
    }
  }

  async function removeSymbol(sym: string) {
    removing = sym;
    addError = null;
    try {
      await instruments.remove(sym);
      const catalog = await data.catalog();
      symbols = catalog.symbols;
      macroSeries = catalog.macro_series;
    } catch (e) {
      addError = String(e);
    } finally {
      removing = null;
    }
  }

  // ── Fetch form handler ───────────────────────────────────────────────────────
  async function runFetch() {
    fetching = true;
    fetchResult = null;
    fetchError = null;
    try {
      if (fetchService === 'bars') {
        const res = await data.ingestBars({ since: fetchSince || undefined, concurrency: 4 });
        fetchResult = res.summary;
        // Refresh catalog
        const catalog = await data.catalog();
        symbols = catalog.symbols;
        macroSeries = catalog.macro_series;
      } else if (fetchService === 'macro') {
        const res = await data.ingestMacro({ since: fetchSince || undefined });
        fetchResult = res.summary;
        const catalog = await data.catalog();
        symbols = catalog.symbols;
        macroSeries = catalog.macro_series;
      } else {
        const res = await data.ingestFundamentals({});
        fetchResult = res.summary;
        const catalog = await data.catalog();
        symbols = catalog.symbols;
        macroSeries = catalog.macro_series;
      }
    } catch (e) {
      fetchError = String(e);
    } finally {
      fetching = false;
    }
  }

  // ── Explorer ──────────────────────────────────────────────────────────────────
  async function loadBars() {
    if (!explorerSymbol.trim()) return;
    barsLoading = true;
    barsError = null;
    try {
      bars = await data.bars(explorerSymbol.trim().toUpperCase(), explorerFrom, explorerTo);
      await tick();
      renderChart();
    } catch (e) {
      barsError = String(e);
      bars = [];
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
      height: 340,
    });
    const cs = chartInstance.addSeries(CandlestickSeries, {
      upColor: '#22c55e',
      downColor: '#ef4444',
      borderVisible: false,
      wickUpColor: '#22c55e',
      wickDownColor: '#ef4444',
    });
    cs.setData(
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

  let barStats = $derived(() => {
    if (bars.length === 0) return null;
    const last = bars[bars.length - 1];
    const prev = bars.length >= 2 ? bars[bars.length - 2] : null;
    const change = prev ? last.close - prev.close : 0;
    const changePct = prev && prev.close ? (change / prev.close) * 100 : 0;
    const avgVol = Math.round(bars.reduce((s, b) => s + b.volume, 0) / bars.length);
    return {
      last,
      change,
      changePct,
      avgVol,
      hi: Math.max(...bars.map(b => b.high)),
      lo: Math.min(...bars.map(b => b.low)),
    };
  });

  // Jump from inventory row to explorer
  function exploreSymbol(sym: string) {
    explorerSymbol = sym;
    activeTab = 'explorer';
    // loadBars() will be triggered by the explorer search button
  }
</script>

<div class="p-6">
  <div class="flex items-center justify-between mb-6">
    <h1 class="text-xl font-semibold" style="color: var(--color-text-primary)">Data</h1>

    <!-- Tabs -->
    <div class="flex rounded-lg overflow-hidden" style="border: 1px solid var(--color-border)">
      {#each [['manager', 'Manager'], ['explorer', 'Explorer']] as [tab, label]}
        <button
          onclick={() => (activeTab = tab as 'manager' | 'explorer')}
          class="px-4 py-1.5 text-sm font-medium"
          style="background: {activeTab === tab ? 'var(--color-accent-blue)' : 'var(--color-bg-card)'}; color: {activeTab === tab ? '#fff' : 'var(--color-text-secondary)'};"
        >
          {label}
        </button>
      {/each}
    </div>
  </div>

  <!-- ══════════════════════════════════════════════════════════ MANAGER TAB -->
  {#if activeTab === 'manager'}
    {#if catalogLoading}
      <div style="color: var(--color-text-muted)">Loading catalog…</div>
    {:else if catalogError}
      <div class="text-sm" style="color: var(--color-accent-red)">{catalogError}</div>
    {:else}
      <!-- Summary KPIs -->
      <div class="grid grid-cols-4 gap-3 mb-5">
        {#each [
          { label: 'Instruments', value: String(symbols.length) },
          { label: 'With Bars', value: String(symbols.filter(s => s.bar_count > 0).length) },
          { label: 'With Fundamentals', value: String(symbols.filter(s => s.income_count > 0).length) },
          { label: 'Macro Series', value: String(macroSeries.length) },
        ] as kpi}
          <div class="rounded-xl p-3" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
            <div class="text-xs mb-1" style="color: var(--color-text-muted)">{kpi.label}</div>
            <div class="text-lg font-semibold" style="color: var(--color-text-primary)">{kpi.value}</div>
          </div>
        {/each}
      </div>

      <div class="grid grid-cols-3 gap-5">
        <!-- Left: inventory table -->
        <div class="col-span-2 rounded-xl overflow-hidden" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
          <div class="flex items-center gap-3 px-4 py-3" style="border-bottom: 1px solid var(--color-border)">
            <span class="text-sm font-medium" style="color: var(--color-text-primary)">Data Inventory</span>
            <input
              type="text"
              placeholder="Search symbol or name…"
              bind:value={inventorySearch}
              class="ml-auto rounded-lg px-3 py-1 text-xs w-44"
              style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;"
            />
          </div>
          <div style="max-height: 420px; overflow-y: auto;">
            <table class="w-full text-xs">
              <thead style="position: sticky; top: 0; background: var(--color-bg-card);">
                <tr style="border-bottom: 1px solid var(--color-border)">
                  <th class="text-left px-4 py-2 font-medium" style="color: var(--color-text-muted)">Symbol</th>
                  <th class="text-left px-4 py-2 font-medium" style="color: var(--color-text-muted)">Name</th>
                  <th class="text-right px-4 py-2 font-medium" style="color: var(--color-text-muted)">Bars</th>
                  <th class="text-left px-4 py-2 font-medium" style="color: var(--color-text-muted)">First</th>
                  <th class="text-left px-4 py-2 font-medium" style="color: var(--color-text-muted)">Last</th>
                  <th class="text-right px-4 py-2 font-medium" style="color: var(--color-text-muted)">IS/BS/CF</th>
                  <th class="px-3 py-2"></th>
                  <th class="px-3 py-2"></th>
                </tr>
              </thead>
              <tbody>
                {#each filteredSymbols as s (s.symbol)}
                  <tr style="border-top: 1px solid var(--color-border)">
                    <td class="px-4 py-1.5 font-medium font-mono" style="color: var(--color-text-primary)">{s.symbol}</td>
                    <td class="px-4 py-1.5 max-w-32 truncate" style="color: var(--color-text-secondary)">{s.name}</td>
                    <td class="px-4 py-1.5 text-right tabular-nums" style="color: {s.bar_count > 0 ? 'var(--color-text-primary)' : 'var(--color-text-muted)'}">{s.bar_count > 0 ? s.bar_count.toLocaleString() : '—'}</td>
                    <td class="px-4 py-1.5" style="color: var(--color-text-muted)">{s.first_bar?.slice(0,10) ?? '—'}</td>
                    <td class="px-4 py-1.5" style="color: var(--color-text-muted)">{s.last_bar?.slice(0,10) ?? '—'}</td>
                    <td class="px-4 py-1.5 text-right tabular-nums" style="color: var(--color-text-muted)">
                      <span style="color: {s.income_count > 0 ? 'var(--color-accent-green)' : 'var(--color-text-muted)'}">{s.income_count > 0 ? '✓' : '—'}</span>/<span style="color: {s.balance_count > 0 ? 'var(--color-accent-green)' : 'var(--color-text-muted)'}">{s.balance_count > 0 ? '✓' : '—'}</span>/<span style="color: {s.cashflow_count > 0 ? 'var(--color-accent-green)' : 'var(--color-text-muted)'}">{s.cashflow_count > 0 ? '✓' : '—'}</span>
                    </td>
                    <td class="px-3 py-1.5">
                      {#if s.bar_count > 0}
                        <button
                          onclick={() => exploreSymbol(s.symbol)}
                          class="text-xs px-2 py-0.5 rounded"
                          style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-secondary);"
                        >Chart</button>
                      {/if}
                    </td>
                    <td class="px-3 py-1.5">
                      <button
                        onclick={() => removeSymbol(s.symbol)}
                        disabled={removing === s.symbol}
                        class="text-xs px-2 py-0.5 rounded"
                        style="color: var(--color-accent-red); background: var(--color-bg-secondary); border: 1px solid var(--color-border); opacity: {removing === s.symbol ? 0.5 : 1};"
                      >✕</button>
                    </td>
                  </tr>
                {:else}
                  <tr>
                    <td colspan="8" class="px-4 py-6 text-center" style="color: var(--color-text-muted)">
                      {inventorySearch ? 'No matches.' : 'No instruments loaded yet.'}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>

          <!-- Macro series table -->
          {#if macroSeries.length > 0}
            <div style="border-top: 2px solid var(--color-border)">
              <div class="px-4 py-2" style="border-bottom: 1px solid var(--color-border)">
                <span class="text-xs font-semibold uppercase tracking-wide" style="color: var(--color-text-muted)">Macro Series</span>
              </div>
              <div class="flex flex-wrap gap-2 px-4 py-3">
                {#each macroSeries as m (m.series_id)}
                  <div class="text-xs rounded-lg px-2 py-1" style="background: var(--color-bg-secondary); border: 1px solid var(--color-border)">
                    <span class="font-mono font-medium" style="color: var(--color-text-primary)">{m.series_id}</span>
                    <span class="ml-1" style="color: var(--color-text-muted)">{m.count.toLocaleString()} pts · {m.first_date?.slice(0,10) ?? '?'} – {m.last_date?.slice(0,10) ?? '?'}</span>
                  </div>
                {/each}
              </div>
            </div>
          {/if}

          <!-- Add symbols form -->
          <div style="border-top: 2px solid var(--color-border)">
            <div class="px-4 py-3 flex items-center gap-3 flex-wrap">
              <span class="text-xs font-semibold uppercase tracking-wide" style="color: var(--color-text-muted)">Add Symbols</span>
              <input
                type="text"
                placeholder="AAPL, MSFT, GOOGL…"
                bind:value={addSymbolsInput}
                onkeydown={(e) => e.key === 'Enter' && addSymbols()}
                class="rounded-lg px-3 py-1.5 text-sm font-mono flex-1 min-w-48"
                style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;"
              />
              <button
                onclick={addSymbols}
                disabled={adding || !addSymbolsInput.trim()}
                class="px-4 py-1.5 rounded-lg text-sm font-medium"
                style="background: var(--color-accent-blue); color: white; opacity: {adding || !addSymbolsInput.trim() ? 0.5 : 1};"
              >
                {adding ? 'Adding…' : 'Add'}
              </button>
            </div>
            {#if addResult}
              <div class="mx-4 mb-3 text-xs rounded-lg px-3 py-2" style="background: rgba(34,197,94,0.1); border: 1px solid rgba(34,197,94,0.3); color: var(--color-accent-green)">
                {addResult}
              </div>
            {/if}
            {#if addError}
              <div class="mx-4 mb-3 text-xs rounded-lg px-3 py-2" style="background: rgba(239,68,68,0.1); border: 1px solid rgba(239,68,68,0.3); color: var(--color-accent-red)">
                {addError}
              </div>
            {/if}
          </div>
        </div>

        <!-- Right: fetch form -->
        <div class="space-y-4">
          <div class="rounded-xl p-4 space-y-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
            <h2 class="text-sm font-medium" style="color: var(--color-text-primary)">Fetch Data</h2>

            <div>
              <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Service</label>
              <select bind:value={fetchService} class="w-full rounded-lg px-3 py-1.5 text-sm"
                style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;">
                <option value="bars">Price Bars (Yahoo Finance)</option>
                <option value="macro">Macro Series (FRED)</option>
                <option value="fundamentals">Fundamentals (EDGAR/SimFin)</option>
              </select>
            </div>

            {#if fetchService === 'bars' || fetchService === 'macro'}
              <div>
                <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Since (optional)</label>
                <input type="date" bind:value={fetchSince} class="w-full rounded-lg px-3 py-1.5 text-sm"
                  style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
                <p class="text-xs mt-1" style="color: var(--color-text-muted)">Leave blank to fetch all available history.</p>
              </div>
            {/if}

            {#if fetchResult}
              <div class="text-xs rounded-lg px-3 py-2" style="background: rgba(34,197,94,0.1); border: 1px solid rgba(34,197,94,0.3); color: var(--color-accent-green)">
                {fetchResult}
              </div>
            {/if}
            {#if fetchError}
              <div class="text-xs rounded-lg px-3 py-2" style="background: rgba(239,68,68,0.1); border: 1px solid rgba(239,68,68,0.3); color: var(--color-accent-red)">
                {fetchError}
              </div>
            {/if}

            <button
              onclick={runFetch}
              disabled={fetching}
              class="w-full py-2 rounded-lg text-sm font-medium"
              style="background: var(--color-accent-blue); color: white; opacity: {fetching ? 0.6 : 1};"
            >
              {fetching ? 'Fetching…' : 'Fetch Now'}
            </button>
          </div>
        </div>
      </div>
    {/if}

  <!-- ═══════════════════════════════════════════════════════ EXPLORER TAB -->
  {:else}
    <!-- Search bar -->
    <div class="flex gap-3 mb-5 items-end flex-wrap">
      <div>
        <label class="block text-xs mb-1" style="color: var(--color-text-muted)">Symbol</label>
        <input
          type="text"
          placeholder="AAPL"
          bind:value={explorerSymbol}
          onkeydown={(e) => e.key === 'Enter' && loadBars()}
          class="rounded-lg px-3 py-1.5 text-sm font-mono w-28"
          style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;"
        />
      </div>
      <div>
        <label class="block text-xs mb-1" style="color: var(--color-text-muted)">From</label>
        <input type="date" bind:value={explorerFrom} class="rounded-lg px-3 py-1.5 text-sm"
          style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
      </div>
      <div>
        <label class="block text-xs mb-1" style="color: var(--color-text-muted)">To</label>
        <input type="date" bind:value={explorerTo} class="rounded-lg px-3 py-1.5 text-sm"
          style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
      </div>
      <button
        onclick={loadBars}
        disabled={barsLoading || !explorerSymbol.trim()}
        class="px-5 py-1.5 rounded-lg text-sm font-medium"
        style="background: var(--color-accent-blue); color: white; opacity: {barsLoading || !explorerSymbol.trim() ? 0.6 : 1};"
      >
        {barsLoading ? 'Loading…' : 'Load'}
      </button>
      {#if bars.length > 0}
        <span class="text-xs self-center ml-auto" style="color: var(--color-text-muted)">{bars.length} bars</span>
      {/if}
    </div>

    {#if barsError}
      <div class="text-sm mb-4" style="color: var(--color-accent-red)">{barsError}</div>
    {/if}

    {#if bars.length > 0}
      {@const stats = barStats()}
      {#if stats}
        <div class="grid grid-cols-6 gap-3 mb-4">
          {#each [
            { label: 'Last Close', value: '$' + fmt(stats.last.close) },
            { label: 'Change', value: (stats.change >= 0 ? '+' : '') + fmt(stats.change) + ' (' + (stats.changePct >= 0 ? '+' : '') + fmt(stats.changePct) + '%)', color: stats.change >= 0 ? 'var(--color-accent-green)' : 'var(--color-accent-red)' },
            { label: 'Period High', value: '$' + fmt(stats.hi), color: 'var(--color-accent-green)' },
            { label: 'Period Low', value: '$' + fmt(stats.lo), color: 'var(--color-accent-red)' },
            { label: 'Avg Volume', value: stats.avgVol.toLocaleString() },
            { label: 'Bars', value: String(bars.length) },
          ] as card}
            <div class="rounded-xl p-3" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
              <div class="text-xs mb-1" style="color: var(--color-text-muted)">{card.label}</div>
              <div class="text-sm font-semibold" style="color: {card.color ?? 'var(--color-text-primary)'}">{card.value}</div>
            </div>
          {/each}
        </div>
      {/if}

      <!-- Candlestick chart -->
      <div class="rounded-xl p-4 mb-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
        <div class="text-sm font-medium mb-3" style="color: var(--color-text-primary)">
          {explorerSymbol.toUpperCase()} · {explorerFrom} → {explorerTo}
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
      <div class="flex items-center justify-center h-64 rounded-xl" style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-muted)">
        Enter a symbol and press Load to explore price history.
      </div>
    {/if}
  {/if}
</div>
