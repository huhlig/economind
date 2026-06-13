<script lang="ts">
  import { onMount } from 'svelte';
  import { settings } from '$lib/api.js';
  import type { LlmSettings, DatafeedSettings, ScheduleSettings, RiskSettings, NotificationsSettings } from '$lib/api.js';
  import { apiKey } from '$lib/stores/auth.js';

  // ── API Key ────────────────────────────────────────────────────────────────
  let newKey = $state('');
  let keySaved = $state(false);

  function saveKey() {
    if (newKey.trim()) {
      apiKey.login(newKey.trim());
      newKey = '';
      keySaved = true;
      setTimeout(() => (keySaved = false), 2000);
    }
  }

  // ── LLM ───────────────────────────────────────────────────────────────────
  let llmLoading = $state(true);
  let llmSaving = $state(false);
  let llmSaved = $state(false);
  let llmError = $state<string | null>(null);
  let llm = $state<LlmSettings>({
    provider: 'auto',
    anthropic_model: 'claude-haiku-4-5',
    local_base_url: 'http://localhost:11434/v1',
    local_model: 'llama3',
    anthropic_api_key_configured: false,
    source: 'default',
  });

  async function loadLlmSettings() {
    llmLoading = true; llmError = null;
    try { llm = await settings.llm(); }
    catch (e) { llmError = readableError(e); }
    finally { llmLoading = false; }
  }

  async function saveLlmSettings() {
    llmSaving = true; llmError = null;
    try {
      llm = await settings.updateLlm({
        provider: llm.provider,
        anthropic_model: llm.anthropic_model,
        local_base_url: llm.local_base_url,
        local_model: llm.local_model,
      });
      llmSaved = true;
      setTimeout(() => (llmSaved = false), 2000);
    } catch (e) { llmError = readableError(e); }
    finally { llmSaving = false; }
  }

  // ── Datafeed ───────────────────────────────────────────────────────────────
  let dfLoading = $state(true);
  let dfSaving = $state(false);
  let dfSaved = $state(false);
  let dfError = $state<string | null>(null);
  let df = $state<DatafeedSettings>({
    bar_concurrency: 4,
    bar_backfill_days: 365,
    fred_series: [],
    alpaca_key_configured: false,
    tiingo_key_configured: false,
    simfin_key_configured: false,
    fred_key_configured: false,
  });
  let fredSeriesText = $state('');

  async function loadDatafeedSettings() {
    dfLoading = true; dfError = null;
    try {
      df = await settings.datafeed();
      fredSeriesText = df.fred_series.join(', ');
    } catch (e) { dfError = readableError(e); }
    finally { dfLoading = false; }
  }

  async function saveDatafeedSettings() {
    dfSaving = true; dfError = null;
    try {
      const series = fredSeriesText.split(',').map(s => s.trim()).filter(Boolean);
      df = await settings.updateDatafeed({
        bar_concurrency: df.bar_concurrency,
        bar_backfill_days: df.bar_backfill_days,
        fred_series: series,
      });
      fredSeriesText = df.fred_series.join(', ');
      dfSaved = true;
      setTimeout(() => (dfSaved = false), 2000);
    } catch (e) { dfError = readableError(e); }
    finally { dfSaving = false; }
  }

  // ── Schedule ───────────────────────────────────────────────────────────────
  let schedLoading = $state(true);
  let schedSaving = $state(false);
  let schedSaved = $state(false);
  let schedError = $state<string | null>(null);
  let sched = $state<ScheduleSettings>({
    enabled: true,
    bars_utc: '22:00',
    macro_utc: '23:00',
    fundamentals_utc: '23:00',
    strategy_utc: '23:30',
    bars_lookback_days: 5,
  });

  async function loadScheduleSettings() {
    schedLoading = true; schedError = null;
    try { sched = await settings.schedule(); }
    catch (e) { schedError = readableError(e); }
    finally { schedLoading = false; }
  }

  async function saveScheduleSettings() {
    schedSaving = true; schedError = null;
    try {
      sched = await settings.updateSchedule(sched);
      schedSaved = true;
      setTimeout(() => (schedSaved = false), 2000);
    } catch (e) { schedError = readableError(e); }
    finally { schedSaving = false; }
  }

  // ── Risk ───────────────────────────────────────────────────────────────────
  let riskLoading = $state(true);
  let riskSaving = $state(false);
  let riskSaved = $state(false);
  let riskError = $state<string | null>(null);
  let risk = $state<RiskSettings>({
    max_drawdown_pct: 0.20,
    max_position_pct: 0.10,
    max_open_positions: 20,
  });

  async function loadRiskSettings() {
    riskLoading = true; riskError = null;
    try { risk = await settings.risk(); }
    catch (e) { riskError = readableError(e); }
    finally { riskLoading = false; }
  }

  async function saveRiskSettings() {
    riskSaving = true; riskError = null;
    try {
      risk = await settings.updateRisk(risk);
      riskSaved = true;
      setTimeout(() => (riskSaved = false), 2000);
    } catch (e) { riskError = readableError(e); }
    finally { riskSaving = false; }
  }

  // ── Notifications ──────────────────────────────────────────────────────────
  let notifLoading = $state(true);
  let notifSaving = $state(false);
  let notifSaved = $state(false);
  let notifError = $state<string | null>(null);
  let notif = $state<NotificationsSettings>({
    webhook_url: null,
    on_signal: false,
    on_run_complete: false,
    on_order: false,
    on_error: true,
  });
  let webhookText = $state('');

  async function loadNotificationsSettings() {
    notifLoading = true; notifError = null;
    try {
      notif = await settings.notifications();
      webhookText = notif.webhook_url ?? '';
    } catch (e) { notifError = readableError(e); }
    finally { notifLoading = false; }
  }

  async function saveNotificationsSettings() {
    notifSaving = true; notifError = null;
    try {
      notif = await settings.updateNotifications({
        ...notif,
        webhook_url: webhookText.trim() || null,
      });
      webhookText = notif.webhook_url ?? '';
      notifSaved = true;
      setTimeout(() => (notifSaved = false), 2000);
    } catch (e) { notifError = readableError(e); }
    finally { notifSaving = false; }
  }

  // ── Shared ─────────────────────────────────────────────────────────────────
  function readableError(e: unknown) {
    const text = e instanceof Error ? e.message : String(e);
    try {
      const parsed = JSON.parse(text);
      return parsed.error ?? text;
    } catch { return text; }
  }

  onMount(() => {
    void loadLlmSettings();
    void loadDatafeedSettings();
    void loadScheduleSettings();
    void loadRiskSettings();
    void loadNotificationsSettings();
  });
</script>

<div class="p-6 max-w-3xl space-y-5">
  <h1 class="text-xl font-semibold" style="color: var(--color-text-primary)">Settings</h1>

  <!-- API Key -->
  <div class="rounded-xl p-5 space-y-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
    <h2 class="text-sm font-medium" style="color: var(--color-text-primary)">API Key</h2>
    <div>
      <label for="current-api-key" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Current key</label>
      <div class="rounded-lg px-3 py-2 text-sm font-mono" style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-muted)">
        <output id="current-api-key">{$apiKey ? '••••••••' + $apiKey.slice(-4) : '(not set)'}</output>
      </div>
    </div>
    <div>
      <label for="replace-api-key" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Replace key</label>
      <input id="replace-api-key" type="password" bind:value={newKey} placeholder="New API key"
        class="w-full rounded-lg px-3 py-2 text-sm mb-3"
        style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
      <div class="flex items-center gap-3">
        <button onclick={saveKey} disabled={!newKey.trim()}
          class="px-5 py-2 rounded-lg text-sm font-medium"
          style="background: var(--color-accent-blue); color: white; opacity: {!newKey.trim() ? 0.5 : 1};">
          Save Key
        </button>
        {#if keySaved}<span class="text-sm" style="color: var(--color-accent-green)">Saved ✓</span>{/if}
      </div>
    </div>
  </div>

  <!-- LLM -->
  <div class="rounded-xl p-5 space-y-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
    <div class="flex items-center justify-between gap-4">
      <h2 class="text-sm font-medium" style="color: var(--color-text-primary)">LLM</h2>
      <span class="rounded px-2 py-1 text-xs"
        style="background: var(--color-bg-secondary); color: {llm.anthropic_api_key_configured ? 'var(--color-accent-green)' : 'var(--color-text-muted)'}; border: 1px solid var(--color-border);">
        Anthropic key {llm.anthropic_api_key_configured ? 'configured' : 'missing'}
      </span>
    </div>
    {#if llmLoading}
      <div class="text-sm" style="color: var(--color-text-muted)">Loading...</div>
    {:else}
      <div class="grid grid-cols-2 gap-4">
        <div>
          <label for="llm-provider" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Provider</label>
          <select id="llm-provider" bind:value={llm.provider} class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;">
            <option value="auto">Auto</option>
            <option value="anthropic">Anthropic</option>
            <option value="local">Local</option>
          </select>
        </div>
        <div>
          <label for="anthropic-model" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Anthropic model</label>
          <input id="anthropic-model" type="text" bind:value={llm.anthropic_model} class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
        </div>
        <div>
          <label for="local-base-url" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Local base URL</label>
          <input id="local-base-url" type="url" bind:value={llm.local_base_url} class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
        </div>
        <div>
          <label for="local-model" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Local model</label>
          <input id="local-model" type="text" bind:value={llm.local_model} class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
        </div>
      </div>
      {#if llmError}<div class="text-sm" style="color: var(--color-accent-red)">{llmError}</div>{/if}
      <div class="flex items-center gap-3">
        <button onclick={saveLlmSettings} disabled={llmSaving} class="px-5 py-2 rounded-lg text-sm font-medium"
          style="background: var(--color-accent-blue); color: white; opacity: {llmSaving ? 0.5 : 1};">
          {llmSaving ? 'Saving...' : 'Save LLM'}
        </button>
        {#if llmSaved}<span class="text-sm" style="color: var(--color-accent-green)">Saved ✓</span>{/if}
      </div>
    {/if}
  </div>

  <!-- Datafeed -->
  <div class="rounded-xl p-5 space-y-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
    <div class="flex items-center justify-between gap-4 flex-wrap">
      <h2 class="text-sm font-medium" style="color: var(--color-text-primary)">Datafeed</h2>
      <div class="flex gap-2 flex-wrap text-xs">
        {#each [['Alpaca', df.alpaca_key_configured], ['Tiingo', df.tiingo_key_configured], ['SimFin', df.simfin_key_configured], ['FRED', df.fred_key_configured]] as [label, ok]}
          <span class="rounded px-2 py-1"
            style="background: var(--color-bg-secondary); color: {ok ? 'var(--color-accent-green)' : 'var(--color-text-muted)'}; border: 1px solid var(--color-border);">
            {label} {ok ? '✓' : '–'}
          </span>
        {/each}
      </div>
    </div>
    {#if dfLoading}
      <div class="text-sm" style="color: var(--color-text-muted)">Loading...</div>
    {:else}
      <div class="grid grid-cols-2 gap-4">
        <div>
          <label for="bar-concurrency" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Bar concurrency</label>
          <input id="bar-concurrency" type="number" min="1" max="32" bind:value={df.bar_concurrency}
            class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
        </div>
        <div>
          <label for="bar-backfill-days" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Bar backfill days</label>
          <input id="bar-backfill-days" type="number" min="1" bind:value={df.bar_backfill_days}
            class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
        </div>
        <div class="col-span-2">
          <label for="fred-series" class="block text-xs mb-1" style="color: var(--color-text-secondary)">FRED series (comma-separated)</label>
          <input id="fred-series" type="text" bind:value={fredSeriesText} placeholder="GDP, UNRATE, FEDFUNDS"
            class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
        </div>
      </div>
      {#if dfError}<div class="text-sm" style="color: var(--color-accent-red)">{dfError}</div>{/if}
      <div class="flex items-center gap-3">
        <button onclick={saveDatafeedSettings} disabled={dfSaving} class="px-5 py-2 rounded-lg text-sm font-medium"
          style="background: var(--color-accent-blue); color: white; opacity: {dfSaving ? 0.5 : 1};">
          {dfSaving ? 'Saving...' : 'Save Datafeed'}
        </button>
        {#if dfSaved}<span class="text-sm" style="color: var(--color-accent-green)">Saved ✓</span>{/if}
      </div>
    {/if}
  </div>

  <!-- Schedule -->
  <div class="rounded-xl p-5 space-y-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
    <div class="flex items-center justify-between gap-4">
      <h2 class="text-sm font-medium" style="color: var(--color-text-primary)">Schedule</h2>
      {#if !schedLoading}
        <label class="flex items-center gap-2 text-xs cursor-pointer" style="color: var(--color-text-secondary)">
          <input type="checkbox" bind:checked={sched.enabled} class="rounded" />
          Enabled
        </label>
      {/if}
    </div>
    {#if schedLoading}
      <div class="text-sm" style="color: var(--color-text-muted)">Loading...</div>
    {:else}
      <div class="grid grid-cols-2 gap-4">
        <div>
          <label for="sched-bars" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Bars ingest (UTC HH:MM)</label>
          <input id="sched-bars" type="text" bind:value={sched.bars_utc} placeholder="22:00"
            class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
        </div>
        <div>
          <label for="sched-macro" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Macro refresh (UTC HH:MM)</label>
          <input id="sched-macro" type="text" bind:value={sched.macro_utc} placeholder="23:00"
            class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
        </div>
        <div>
          <label for="sched-fund" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Fundamentals (UTC HH:MM)</label>
          <input id="sched-fund" type="text" bind:value={sched.fundamentals_utc} placeholder="23:00"
            class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
        </div>
        <div>
          <label for="sched-strat" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Strategy run (UTC HH:MM)</label>
          <input id="sched-strat" type="text" bind:value={sched.strategy_utc} placeholder="23:30"
            class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
        </div>
        <div>
          <label for="sched-lookback" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Nightly bars lookback (days)</label>
          <input id="sched-lookback" type="number" min="1" bind:value={sched.bars_lookback_days}
            class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
        </div>
      </div>
      {#if schedError}<div class="text-sm" style="color: var(--color-accent-red)">{schedError}</div>{/if}
      <div class="flex items-center gap-3">
        <button onclick={saveScheduleSettings} disabled={schedSaving} class="px-5 py-2 rounded-lg text-sm font-medium"
          style="background: var(--color-accent-blue); color: white; opacity: {schedSaving ? 0.5 : 1};">
          {schedSaving ? 'Saving...' : 'Save Schedule'}
        </button>
        {#if schedSaved}<span class="text-sm" style="color: var(--color-accent-green)">Saved ✓</span>{/if}
      </div>
    {/if}
  </div>

  <!-- Risk -->
  <div class="rounded-xl p-5 space-y-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
    <h2 class="text-sm font-medium" style="color: var(--color-text-primary)">Risk Controls</h2>
    {#if riskLoading}
      <div class="text-sm" style="color: var(--color-text-muted)">Loading...</div>
    {:else}
      <div class="grid grid-cols-3 gap-4">
        <div>
          <label for="risk-drawdown" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Max drawdown (%)</label>
          <input id="risk-drawdown" type="number" min="0" max="100" step="1"
            value={Math.round(risk.max_drawdown_pct * 100)}
            oninput={(e) => { risk.max_drawdown_pct = Number((e.target as HTMLInputElement).value) / 100; }}
            class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
        </div>
        <div>
          <label for="risk-position" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Max position size (%)</label>
          <input id="risk-position" type="number" min="0" max="100" step="1"
            value={Math.round(risk.max_position_pct * 100)}
            oninput={(e) => { risk.max_position_pct = Number((e.target as HTMLInputElement).value) / 100; }}
            class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
        </div>
        <div>
          <label for="risk-open" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Max open positions</label>
          <input id="risk-open" type="number" min="1" bind:value={risk.max_open_positions}
            class="w-full rounded-lg px-3 py-2 text-sm"
            style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
        </div>
      </div>
      {#if riskError}<div class="text-sm" style="color: var(--color-accent-red)">{riskError}</div>{/if}
      <div class="flex items-center gap-3">
        <button onclick={saveRiskSettings} disabled={riskSaving} class="px-5 py-2 rounded-lg text-sm font-medium"
          style="background: var(--color-accent-blue); color: white; opacity: {riskSaving ? 0.5 : 1};">
          {riskSaving ? 'Saving...' : 'Save Risk'}
        </button>
        {#if riskSaved}<span class="text-sm" style="color: var(--color-accent-green)">Saved ✓</span>{/if}
      </div>
    {/if}
  </div>

  <!-- Notifications -->
  <div class="rounded-xl p-5 space-y-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
    <h2 class="text-sm font-medium" style="color: var(--color-text-primary)">Notifications</h2>
    {#if notifLoading}
      <div class="text-sm" style="color: var(--color-text-muted)">Loading...</div>
    {:else}
      <div>
        <label for="webhook-url" class="block text-xs mb-1" style="color: var(--color-text-secondary)">Webhook URL (Discord / Slack)</label>
        <input id="webhook-url" type="url" bind:value={webhookText} placeholder="https://hooks.slack.com/..."
          class="w-full rounded-lg px-3 py-2 text-sm"
          style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
      </div>
      <div class="grid grid-cols-2 gap-3">
        {#each [
          ['on_signal', 'On signal emitted'] as const,
          ['on_run_complete', 'On strategy run complete'] as const,
          ['on_order', 'On order submitted'] as const,
          ['on_error', 'On error'] as const,
        ] as [key, label]}
          <label class="flex items-center gap-2 text-sm cursor-pointer" style="color: var(--color-text-secondary)">
            <input type="checkbox" bind:checked={notif[key]} class="rounded" />
            {label}
          </label>
        {/each}
      </div>
      {#if notifError}<div class="text-sm" style="color: var(--color-accent-red)">{notifError}</div>{/if}
      <div class="flex items-center gap-3">
        <button onclick={saveNotificationsSettings} disabled={notifSaving} class="px-5 py-2 rounded-lg text-sm font-medium"
          style="background: var(--color-accent-blue); color: white; opacity: {notifSaving ? 0.5 : 1};">
          {notifSaving ? 'Saving...' : 'Save Notifications'}
        </button>
        {#if notifSaved}<span class="text-sm" style="color: var(--color-accent-green)">Saved ✓</span>{/if}
      </div>
    {/if}
  </div>

  <!-- Session -->
  <div class="rounded-xl p-5" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
    <h2 class="text-sm font-medium mb-4" style="color: var(--color-text-primary)">Session</h2>
    <button onclick={() => apiKey.logout()} class="px-5 py-2 rounded-lg text-sm font-medium"
      style="background: #7f1d1d; color: #fca5a5; border: 1px solid #991b1b;">
      Sign Out
    </button>
    <p class="text-xs mt-2" style="color: var(--color-text-muted)">
      Clears your API key from local storage and disconnects the WebSocket.
    </p>
  </div>

  <!-- About -->
  <div class="rounded-xl p-5" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
    <h2 class="text-sm font-medium mb-2" style="color: var(--color-text-primary)">About</h2>
    <p class="text-xs" style="color: var(--color-text-muted)">
      Economind — Low Frequency Trading Analysis Platform<br />
      SvelteKit dashboard · Rust/Axum backend · Lightweight Charts
    </p>
  </div>
</div>
