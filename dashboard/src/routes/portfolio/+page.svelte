<script lang="ts">
  import { onMount } from 'svelte';
  import { portfolio, data as dataApi, instruments } from '$lib/api.js';
  import type { PortfolioSummary, WatchItem } from '$lib/api.js';
  import { eventLog } from '$lib/stores/events.js';
  import { pageContext } from '$lib/stores/pageContext.js';

  interface WatchItemEnriched extends WatchItem {
    last_price?: number;
    price_change?: number;
    price_change_pct?: number;
    volume?: number;
  }

  let data = $state<PortfolioSummary | null>(null);
  let watches = $state<WatchItemEnriched[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  // ── Buy modal ──────────────────────────────────────────────────────────────
  let showBuy = $state(false);
  let buySymbol = $state('');
  let buyShares = $state('');
  let buyPrice = $state('');
  let buyDate = $state(new Date().toISOString().slice(0, 16));
  let buyError = $state<string | null>(null);
  let buyLoading = $state(false);

  // ── Sell modal ─────────────────────────────────────────────────────────────
  let showSell = $state(false);
  let sellPositionId = $state('');
  let sellSymbol = $state('');
  let sellPrice = $state('');
  let sellDate = $state(new Date().toISOString().slice(0, 16));
  let sellError = $state<string | null>(null);
  let sellLoading = $state(false);

  // ── Cash modal ─────────────────────────────────────────────────────────────
  let showCash = $state(false);
  let cashAmount = $state('');
  let cashError = $state<string | null>(null);
  let cashLoading = $state(false);

  function openCashModal() {
    cashAmount = data ? String(data.cash) : '';
    cashError = null;
    showCash = true;
  }

  async function submitCash() {
    cashError = null;
    cashLoading = true;
    try {
      await portfolio.setCash(String(cashAmount).trim());
      showCash = false;
      await load();
    } catch (e) {
      cashError = String(e);
    } finally {
      cashLoading = false;
    }
  }

  // ── Watch add ──────────────────────────────────────────────────────────────
  let watchSymbol = $state('');
  let watchError = $state<string | null>(null);

  async function load() {
    try {
      const [summary, ws] = await Promise.all([portfolio.summary(), portfolio.listWatches()]);
      data = summary;
      watches = ws;
      // Enrich watches with latest bar data (fire-and-forget per symbol)
      const to = new Date().toISOString().slice(0, 10);
      const from = new Date(Date.now() - 10 * 86400_000).toISOString().slice(0, 10);
      ws.forEach(async (w) => {
        try {
          const bars = await dataApi.bars(w.symbol, from, to);
          if (bars.length >= 1) {
            const last = bars[bars.length - 1];
            const prev = bars.length >= 2 ? bars[bars.length - 2] : null;
            const change = prev ? last.close - prev.close : undefined;
            const changePct = prev && prev.close ? change! / prev.close : undefined;
            watches = watches.map(x =>
              x.symbol === w.symbol
                ? { ...x, last_price: last.close, price_change: change, price_change_pct: changePct, volume: last.volume }
                : x
            );
          }
        } catch { /* no bar data available */ }
      });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  onMount(load);

  $effect(() => {
    const latest = $eventLog[0];
    if (latest?.event.type === 'position_opened' || latest?.event.type === 'position_closed') {
      load();
    }
  });

  $effect(() => {
    if (data) {
      pageContext.set(
        `[Portfolio screen]\n` +
        `- Cash: $${fmt(data.cash)}\n` +
        `- Total Equity: $${fmt(data.total_equity)}\n` +
        `- Unrealized P&L: ${data.unrealized_pnl >= 0 ? '+' : ''}$${fmt(Math.abs(data.unrealized_pnl))}\n` +
        `- Open Positions (${data.positions.length}): ${data.positions.map(p => `${p.symbol} ${p.quantity}sh @ $${fmt(p.average_cost)}`).join(', ') || 'none'}\n` +
        `- Watchlist (${watches.length} symbols): ${watches.map(w => w.symbol).join(', ') || 'none'}`
      );
    }
    return () => pageContext.set('');
  });

  function fmt(n: number, d = 2) {
    return n.toLocaleString('en-US', { minimumFractionDigits: d, maximumFractionDigits: d });
  }

  function pnlColor(n: number) {
    return n >= 0 ? 'var(--color-accent-green)' : 'var(--color-accent-red)';
  }

  function openBuyModal() {
    buySymbol = '';
    buyShares = '';
    buyPrice = '';
    buyDate = new Date().toISOString().slice(0, 16);
    buyError = null;
    showBuy = true;
  }

  function openSellModal(posId: string, symbol: string) {
    sellPositionId = posId;
    sellSymbol = symbol;
    sellPrice = '';
    sellDate = new Date().toISOString().slice(0, 16);
    sellError = null;
    showSell = true;
  }

  async function submitBuy() {
    buyError = null;
    buyLoading = true;
    const sym = String(buySymbol).trim().toUpperCase();
    try {
      await Promise.all([
        portfolio.buy({
          symbol: sym,
          shares: String(buyShares).trim(),
          entry_price: String(buyPrice).trim(),
          entry_at: buyDate ? new Date(buyDate).toISOString() : undefined,
        }),
        instruments.add(sym),
      ]);
      showBuy = false;
      await load();
    } catch (e) {
      buyError = String(e);
    } finally {
      buyLoading = false;
    }
  }

  async function submitSell() {
    sellError = null;
    sellLoading = true;
    try {
      await portfolio.sell(sellPositionId, {
        exit_price: String(sellPrice).trim(),
        exit_at: sellDate ? new Date(sellDate).toISOString() : undefined,
      });
      showSell = false;
      await load();
    } catch (e) {
      sellError = String(e);
    } finally {
      sellLoading = false;
    }
  }

  async function addWatch() {
    watchError = null;
    const sym = watchSymbol.trim().toUpperCase();
    if (!sym) return;
    try {
      const [item] = await Promise.all([portfolio.addWatch(sym), instruments.add(sym)]);
      watches = [item, ...watches.filter(w => w.symbol !== sym)];
      watchSymbol = '';
      // Enrich the newly added symbol
      const to = new Date().toISOString().slice(0, 10);
      const from = new Date(Date.now() - 10 * 86400_000).toISOString().slice(0, 10);
      try {
        const bars = await dataApi.bars(sym, from, to);
        if (bars.length >= 1) {
          const last = bars[bars.length - 1];
          const prev = bars.length >= 2 ? bars[bars.length - 2] : null;
          const change = prev ? last.close - prev.close : undefined;
          const changePct = prev && prev.close ? change! / prev.close : undefined;
          watches = watches.map(x =>
            x.symbol === sym
              ? { ...x, last_price: last.close, price_change: change, price_change_pct: changePct, volume: last.volume }
              : x
          );
        }
      } catch { /* no bar data */ }
    } catch (e) {
      watchError = String(e);
    }
  }

  async function removeWatch(sym: string) {
    try {
      await portfolio.removeWatch(sym);
      watches = watches.filter(w => w.symbol !== sym);
    } catch (e) {
      watchError = String(e);
    }
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
      <div class="rounded-xl p-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
        <div class="text-xs mb-1" style="color: var(--color-text-muted)">Total Equity</div>
        <div class="text-xl font-semibold" style="color: var(--color-text-primary)">${fmt(data.total_equity)}</div>
      </div>
      <button
        onclick={openCashModal}
        class="rounded-xl p-4 text-left"
        style="background: var(--color-bg-card); border: 1px solid var(--color-border); cursor: pointer;"
      >
        <div class="text-xs mb-1 flex items-center gap-1" style="color: var(--color-text-muted)">
          Cash <span class="text-xs opacity-60">(click to edit)</span>
        </div>
        <div class="text-xl font-semibold" style="color: var(--color-text-primary)">${fmt(data.cash)}</div>
      </button>
      <div class="rounded-xl p-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
        <div class="text-xs mb-1" style="color: var(--color-text-muted)">Unrealized P&amp;L</div>
        <div class="text-xl font-semibold" style="color: {pnlColor(data.unrealized_pnl)}">
          {data.unrealized_pnl >= 0 ? '+$' : '-$'}{fmt(Math.abs(data.unrealized_pnl))}
        </div>
      </div>
    </div>

    <!-- Open Positions -->
    <div class="rounded-xl overflow-hidden mb-8" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
      <div class="px-4 py-3 flex items-center justify-between" style="border-bottom: 1px solid var(--color-border)">
        <span class="text-sm font-medium" style="color: var(--color-text-primary)">Open Positions ({data.positions.length})</span>
        <button
          onclick={openBuyModal}
          class="text-xs px-3 py-1.5 rounded-lg font-medium"
          style="background: var(--color-accent-green); color: #fff;"
        >
          + Buy
        </button>
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
              <th class="px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)"></th>
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
                <td class="px-4 py-2.5 text-right">
                  <button
                    onclick={() => openSellModal(pos.id, pos.symbol)}
                    class="text-xs px-2.5 py-1 rounded font-medium"
                    style="background: var(--color-accent-red); color: #fff;"
                  >
                    Sell
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>

    <!-- Watchlist -->
    <div class="rounded-xl overflow-hidden" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
      <div class="px-4 py-3" style="border-bottom: 1px solid var(--color-border)">
        <span class="text-sm font-medium" style="color: var(--color-text-primary)">Watchlist ({watches.length})</span>
      </div>
      <!-- Add watch form -->
      <div class="px-4 py-3 flex gap-2" style="border-bottom: 1px solid var(--color-border)">
        <input
          type="text"
          bind:value={watchSymbol}
          placeholder="Symbol (e.g. AAPL)"
          class="text-sm px-3 py-1.5 rounded-lg flex-1"
          style="background: var(--color-bg-input, var(--color-bg-secondary)); border: 1px solid var(--color-border); color: var(--color-text-primary);"
          onkeydown={(e) => e.key === 'Enter' && addWatch()}
        />
        <button
          onclick={addWatch}
          class="text-xs px-3 py-1.5 rounded-lg font-medium"
          style="background: var(--color-accent); color: #fff;"
        >
          Add
        </button>
      </div>
      {#if watchError}
        <div class="px-4 py-2 text-xs" style="color: var(--color-accent-red)">{watchError}</div>
      {/if}
      {#if watches.length === 0}
        <p class="px-4 py-6 text-sm text-center" style="color: var(--color-text-muted)">No symbols being watched.</p>
      {:else}
        <table class="w-full text-sm">
          <thead>
            <tr style="border-bottom: 1px solid var(--color-border)">
              <th class="text-left px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Symbol</th>
              <th class="text-right px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Last Price</th>
              <th class="text-right px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Change</th>
              <th class="text-right px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Volume</th>
              <th class="text-right px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)">Added</th>
              <th class="px-4 py-3 text-xs font-medium" style="color: var(--color-text-muted)"></th>
            </tr>
          </thead>
          <tbody>
            {#each watches as w}
              <tr style="border-top: 1px solid var(--color-border)">
                <td class="px-4 py-2.5 font-medium" style="color: var(--color-text-primary)">{w.symbol}</td>
                <td class="px-4 py-2.5 text-right" style="color: var(--color-text-secondary)">
                  {w.last_price != null ? '$' + fmt(w.last_price) : '—'}
                </td>
                <td class="px-4 py-2.5 text-right font-medium" style="color: {w.price_change != null ? (w.price_change >= 0 ? 'var(--color-accent-green)' : 'var(--color-accent-red)') : 'var(--color-text-muted)'}">
                  {#if w.price_change != null && w.price_change_pct != null}
                    {w.price_change >= 0 ? '+' : ''}{fmt(w.price_change)} ({w.price_change >= 0 ? '+' : ''}{fmt(w.price_change_pct * 100)}%)
                  {:else}
                    —
                  {/if}
                </td>
                <td class="px-4 py-2.5 text-right text-xs" style="color: var(--color-text-muted)">
                  {w.volume != null ? w.volume.toLocaleString() : '—'}
                </td>
                <td class="px-4 py-2.5 text-right text-xs" style="color: var(--color-text-muted)">
                  {new Date(w.added_at).toLocaleDateString()}
                </td>
                <td class="px-4 py-2.5 text-right">
                  <button
                    onclick={() => removeWatch(w.symbol)}
                    class="text-xs px-2 py-1 rounded"
                    style="color: var(--color-text-muted); border: 1px solid var(--color-border);"
                  >
                    Remove
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>
  {/if}
</div>

<!-- Cash Modal -->
{#if showCash}
  <div
    class="fixed inset-0 flex items-center justify-center z-50"
    style="background: rgba(0,0,0,0.5)"
    role="dialog"
    aria-modal="true"
  >
    <div class="rounded-xl p-6 w-80" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
      <h2 class="text-base font-semibold mb-4" style="color: var(--color-text-primary)">Update Cash Balance</h2>
      {#if cashError}
        <div class="text-sm mb-3" style="color: var(--color-accent-red)">{cashError}</div>
      {/if}
      <label class="block text-xs mb-1" style="color: var(--color-text-muted)">Cash ($)</label>
      <input
        type="number"
        min="0"
        step="0.01"
        bind:value={cashAmount}
        class="w-full rounded-lg px-3 py-2 text-sm mb-4"
        style="background: var(--color-bg-input); border: 1px solid var(--color-border); color: var(--color-text-primary)"
        placeholder="e.g. 10000.00"
      />
      <div class="flex gap-2 justify-end">
        <button
          onclick={() => (showCash = false)}
          class="text-xs px-3 py-1.5 rounded-lg"
          style="background: var(--color-bg-hover); color: var(--color-text-muted)"
        >Cancel</button>
        <button
          onclick={submitCash}
          disabled={cashLoading}
          class="text-xs px-3 py-1.5 rounded-lg font-medium"
          style="background: var(--color-accent-blue); color: #fff"
        >{cashLoading ? 'Saving…' : 'Save'}</button>
      </div>
    </div>
  </div>
{/if}

<!-- Buy Modal -->
{#if showBuy}
  <div
    class="fixed inset-0 flex items-center justify-center z-50"
    style="background: rgba(0,0,0,0.5)"
    role="dialog"
    aria-modal="true"
  >
    <div class="rounded-xl p-6 w-full max-w-md" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
      <h2 class="text-base font-semibold mb-4" style="color: var(--color-text-primary)">Buy Position</h2>

      <div class="space-y-3">
        <div>
          <label class="text-xs mb-1 block" style="color: var(--color-text-muted)">Symbol</label>
          <input
            type="text"
            bind:value={buySymbol}
            placeholder="AAPL"
            class="w-full text-sm px-3 py-2 rounded-lg"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary);"
          />
        </div>
        <div>
          <label class="text-xs mb-1 block" style="color: var(--color-text-muted)">Shares</label>
          <input
            type="number"
            bind:value={buyShares}
            placeholder="100"
            min="0"
            step="any"
            class="w-full text-sm px-3 py-2 rounded-lg"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary);"
          />
        </div>
        <div>
          <label class="text-xs mb-1 block" style="color: var(--color-text-muted)">Entry Price ($)</label>
          <input
            type="number"
            bind:value={buyPrice}
            placeholder="150.00"
            min="0"
            step="any"
            class="w-full text-sm px-3 py-2 rounded-lg"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary);"
          />
        </div>
        <div>
          <label class="text-xs mb-1 block" style="color: var(--color-text-muted)">Date & Time</label>
          <input
            type="datetime-local"
            bind:value={buyDate}
            class="w-full text-sm px-3 py-2 rounded-lg"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary);"
          />
        </div>
      </div>

      {#if buyError}
        <div class="mt-3 text-xs" style="color: var(--color-accent-red)">{buyError}</div>
      {/if}

      <div class="flex gap-3 mt-5">
        <button
          onclick={() => (showBuy = false)}
          class="flex-1 text-sm py-2 rounded-lg"
          style="border: 1px solid var(--color-border); color: var(--color-text-secondary);"
        >
          Cancel
        </button>
        <button
          onclick={submitBuy}
          disabled={buyLoading}
          class="flex-1 text-sm py-2 rounded-lg font-medium"
          style="background: var(--color-accent-green); color: #fff; opacity: {buyLoading ? 0.6 : 1};"
        >
          {buyLoading ? 'Buying…' : 'Confirm Buy'}
        </button>
      </div>
    </div>
  </div>
{/if}

<!-- Sell Modal -->
{#if showSell}
  <div
    class="fixed inset-0 flex items-center justify-center z-50"
    style="background: rgba(0,0,0,0.5)"
    role="dialog"
    aria-modal="true"
  >
    <div class="rounded-xl p-6 w-full max-w-md" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
      <h2 class="text-base font-semibold mb-4" style="color: var(--color-text-primary)">
        Sell Position — <span style="color: var(--color-accent-red)">{sellSymbol}</span>
      </h2>

      <div class="space-y-3">
        <div>
          <label class="text-xs mb-1 block" style="color: var(--color-text-muted)">Exit Price ($)</label>
          <input
            type="number"
            bind:value={sellPrice}
            placeholder="155.00"
            min="0"
            step="any"
            class="w-full text-sm px-3 py-2 rounded-lg"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary);"
          />
        </div>
        <div>
          <label class="text-xs mb-1 block" style="color: var(--color-text-muted)">Date & Time</label>
          <input
            type="datetime-local"
            bind:value={sellDate}
            class="w-full text-sm px-3 py-2 rounded-lg"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary);"
          />
        </div>
      </div>

      {#if sellError}
        <div class="mt-3 text-xs" style="color: var(--color-accent-red)">{sellError}</div>
      {/if}

      <div class="flex gap-3 mt-5">
        <button
          onclick={() => (showSell = false)}
          class="flex-1 text-sm py-2 rounded-lg"
          style="border: 1px solid var(--color-border); color: var(--color-text-secondary);"
        >
          Cancel
        </button>
        <button
          onclick={submitSell}
          disabled={sellLoading}
          class="flex-1 text-sm py-2 rounded-lg font-medium"
          style="background: var(--color-accent-red); color: #fff; opacity: {sellLoading ? 0.6 : 1};"
        >
          {sellLoading ? 'Selling…' : 'Confirm Sell'}
        </button>
      </div>
    </div>
  </div>
{/if}
