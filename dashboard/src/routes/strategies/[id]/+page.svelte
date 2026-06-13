<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { strategy } from '$lib/api.js';
  import type { StrategyConfig } from '$lib/api.js';

  let cfg = $state<StrategyConfig | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let saving = $state(false);
  let saved = $state(false);

  // Editable fields
  let editName = $state('');
  let editDescription = $state('');
  let editParams = $state('');
  let editUniverse = $state('');

  onMount(async () => {
    const id = $page.params.id;
    try {
      cfg = await strategy.get(id);
      editName = cfg.name;
      editDescription = cfg.description ?? '';
      editParams = JSON.stringify(cfg.parameters, null, 2);
      editUniverse = cfg.universe.join(', ');
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  async function save() {
    if (!cfg) return;
    saving = true;
    try {
      let parameters: Record<string, string>;
      try {
        parameters = JSON.parse(editParams) as Record<string, string>;
      } catch {
        alert('Parameters must be valid JSON');
        return;
      }
      const universe = editUniverse.split(',').map(s => s.trim()).filter(Boolean);
      cfg = await strategy.update(cfg.id, {
        name: editName,
        description: editDescription || undefined,
        parameters,
        universe,
      });
      saved = true;
      setTimeout(() => (saved = false), 2000);
    } catch (e) {
      alert('Save failed: ' + String(e));
    } finally {
      saving = false;
    }
  }
</script>

<div class="p-6 max-w-2xl">
  <div class="flex items-center gap-2 mb-6">
    <a href="/strategies" class="text-sm" style="color: var(--color-text-muted)">← Strategies</a>
  </div>

  {#if loading}
    <div style="color: var(--color-text-muted)">Loading…</div>
  {:else if error}
    <div class="text-sm" style="color: var(--color-accent-red)">{error}</div>
  {:else if cfg}
    <h1 class="text-xl font-semibold mb-6" style="color: var(--color-text-primary)">Edit Strategy</h1>

    <div class="space-y-5">
      <div>
        <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Name</label>
        <input
          type="text"
          bind:value={editName}
          class="w-full rounded-lg px-3 py-2 text-sm"
          style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;"
        />
      </div>

      <div>
        <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Description</label>
        <input
          type="text"
          bind:value={editDescription}
          class="w-full rounded-lg px-3 py-2 text-sm"
          style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;"
        />
      </div>

      <div>
        <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Universe (comma-separated symbols)</label>
        <input
          type="text"
          bind:value={editUniverse}
          class="w-full rounded-lg px-3 py-2 text-sm font-mono"
          style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;"
          placeholder="AAPL, MSFT, GOOGL"
        />
      </div>

      <div>
        <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Parameters (JSON)</label>
        <textarea
          bind:value={editParams}
          rows="8"
          class="w-full rounded-lg px-3 py-2 text-sm font-mono"
          style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none; resize: vertical;"
        ></textarea>
      </div>

      <!-- Plugins (read-only) -->
      <div>
        <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Plugins</label>
        <div class="flex flex-wrap gap-2">
          {#each cfg.plugins as p}
            <span class="text-xs px-2.5 py-1 rounded-lg" style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-secondary)">
              {p.role}: {p.name}
            </span>
          {/each}
        </div>
        <p class="text-xs mt-1" style="color: var(--color-text-muted)">Plugin configuration is managed server-side.</p>
      </div>

      <div class="flex items-center gap-3 pt-2">
        <button
          onclick={save}
          disabled={saving}
          class="px-5 py-2 rounded-lg text-sm font-medium"
          style="background: var(--color-accent-blue); color: white; opacity: {saving ? 0.6 : 1};"
        >
          {saving ? 'Saving…' : 'Save Changes'}
        </button>
        {#if saved}
          <span class="text-sm" style="color: var(--color-accent-green)">Saved ✓</span>
        {/if}
      </div>
    </div>
  {/if}
</div>
