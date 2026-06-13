<script lang="ts">
  import { onMount } from 'svelte';
  import { strategy } from '$lib/api.js';
  import type { StrategyConfig, StrategyRunResult } from '$lib/api.js';

  let configs = $state<StrategyConfig[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let running = $state<Set<string>>(new Set());
  let runResults = $state<Record<string, StrategyRunResult>>({});

  onMount(async () => {
    try {
      configs = await strategy.list();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  async function triggerRun(id: string) {
    running = new Set([...running, id]);
    try {
      const result = await strategy.run(id);
      runResults = { ...runResults, [id]: result };
    } catch (e) {
      alert('Run failed: ' + String(e));
    } finally {
      running = new Set([...running].filter(x => x !== id));
    }
  }

  async function toggleEnabled(cfg: StrategyConfig) {
    try {
      const updated = await strategy.update(cfg.id, { enabled: !cfg.enabled });
      configs = configs.map(c => c.id === updated.id ? updated : c);
    } catch (e) {
      alert('Update failed: ' + String(e));
    }
  }
</script>

<div class="p-6">
  <h1 class="text-xl font-semibold mb-6" style="color: var(--color-text-primary)">Strategy Manager</h1>

  {#if loading}
    <div style="color: var(--color-text-muted)">Loading…</div>
  {:else if error}
    <div class="text-sm" style="color: var(--color-accent-red)">{error}</div>
  {:else if configs.length === 0}
    <p class="text-sm" style="color: var(--color-text-muted)">No strategy configurations found.</p>
  {:else}
    <div class="space-y-4">
      {#each configs as cfg (cfg.id)}
        <div class="rounded-xl p-5" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
          <div class="flex items-start justify-between mb-3">
            <div>
              <div class="flex items-center gap-2">
                <span class="font-medium" style="color: var(--color-text-primary)">{cfg.name}</span>
                <span
                  class="text-xs px-2 py-0.5 rounded-full"
                  style="background: {cfg.enabled ? '#166534' : '#374151'}; color: {cfg.enabled ? 'var(--color-accent-green)' : 'var(--color-text-muted)'}"
                >
                  {cfg.enabled ? 'Active' : 'Disabled'}
                </span>
              </div>
              {#if cfg.description}
                <p class="text-xs mt-1" style="color: var(--color-text-muted)">{cfg.description}</p>
              {/if}
            </div>
            <div class="flex gap-2">
              <button
                onclick={() => toggleEnabled(cfg)}
                class="text-xs px-3 py-1.5 rounded-lg"
                style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-secondary)"
              >
                {cfg.enabled ? 'Disable' : 'Enable'}
              </button>
              <a
                href="/strategies/{cfg.id}"
                class="text-xs px-3 py-1.5 rounded-lg"
                style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-secondary)"
              >
                Edit
              </a>
              <button
                onclick={() => triggerRun(cfg.id)}
                disabled={running.has(cfg.id)}
                class="text-xs px-3 py-1.5 rounded-lg font-medium"
                style="background: var(--color-accent-blue); color: white; opacity: {running.has(cfg.id) ? 0.6 : 1};"
              >
                {running.has(cfg.id) ? 'Running…' : 'Run Now'}
              </button>
            </div>
          </div>

          <!-- Universe -->
          <div class="flex flex-wrap gap-1.5 mb-3">
            {#each cfg.universe as sym}
              <span class="text-xs px-2 py-0.5 rounded" style="background: var(--color-bg-secondary); color: var(--color-text-secondary)">
                {sym}
              </span>
            {/each}
          </div>

          <!-- Plugins -->
          <div class="flex gap-2 text-xs">
            {#each cfg.plugins as p}
              <span class="px-2 py-0.5 rounded" style="background: var(--color-bg-primary); border: 1px solid var(--color-border); color: var(--color-text-muted)">
                {p.role}: {p.name}
              </span>
            {/each}
          </div>

          <!-- Run result feedback -->
          {#if runResults[cfg.id]}
            {@const r = runResults[cfg.id]}
            <div class="mt-3 text-xs rounded px-3 py-2" style="background: var(--color-bg-secondary); border: 1px solid var(--color-border)">
              <span style="color: var(--color-text-muted)">Last run: </span>
              <span style="color: {r.status === 'completed' ? 'var(--color-accent-green)' : 'var(--color-accent-red)'}">
                {r.status}
              </span>
              {#if r.signals_generated != null}
                <span style="color: var(--color-text-muted)"> · {r.signals_generated} signals</span>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
