<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { strategy } from '$lib/api.js';
  import type { StrategyConfig } from '$lib/api.js';
  import {
    PLUGIN_REGISTRY, PLUGIN_ROLES, pluginsByRole, findPlugin, defaultParamsFor,
  } from '$lib/pluginRegistry.js';
  import type { PluginRole } from '$lib/pluginRegistry.js';

  let cfg = $state<StrategyConfig | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let saving = $state(false);
  let saved = $state(false);
  let saveError = $state<string | null>(null);

  let editName = $state('');
  let editDescription = $state('');
  let editComposition = $state('pipeline');
  let editEnabled = $state(true);
  let editUniverse = $state('');

  // Each plugin has its own inline params
  interface KVRow { key: string; value: string }
  interface PluginRow { role: string; name: string; params: KVRow[] }
  let plugins = $state<PluginRow[]>([]);

  // Distribute flat params dict to plugins by matching registry keys
  function distributeParams(flatParams: Record<string, string>, pluginList: { role: string; name: string }[]): PluginRow[] {
    // Build a map: paramKey -> plugin index (first plugin whose registry defines that key)
    const keyToPlugin = new Map<string, number>();
    pluginList.forEach((p, i) => {
      const def = findPlugin(p.name);
      if (def) def.params.forEach(param => { if (!keyToPlugin.has(param.key)) keyToPlugin.set(param.key, i); });
    });

    const rows: KVRow[][] = pluginList.map(() => []);
    const unmatched: KVRow[] = [];

    for (const [key, value] of Object.entries(flatParams)) {
      const idx = keyToPlugin.get(key);
      if (idx !== undefined) rows[idx].push({ key, value });
      else unmatched.push({ key, value });
    }

    // Unmatched params go to first plugin (or stay if no plugins)
    if (unmatched.length > 0 && rows.length > 0) rows[0].push(...unmatched);

    return pluginList.map((p, i) => ({
      role: p.role,
      name: p.name,
      params: rows[i].length > 0 ? rows[i] : [],
    }));
  }

  onMount(async () => {
    const id = $page.params.id;
    if (!id) { error = 'Strategy id is missing.'; loading = false; return; }
    try {
      cfg = await strategy.get(id);
      editName = cfg.name;
      editDescription = cfg.description ?? '';
      editComposition = cfg.composition ?? 'pipeline';
      editEnabled = cfg.enabled;
      editUniverse = (cfg.universe ?? []).join(', ');

      const rawPlugins = cfg.plugins.length > 0
        ? cfg.plugins.map(p => ({ role: p.role, name: p.name }))
        : [{ role: 'identifier', name: '' }];

      plugins = distributeParams(cfg.parameters ?? {}, rawPlugins);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  function addPlugin() {
    plugins = [...plugins, { role: 'identifier', name: '', params: [] }];
  }

  function removePlugin(i: number) {
    plugins = plugins.filter((_, idx) => idx !== i);
  }

  function onPluginNameChange(i: number, name: string) {
    const def = findPlugin(name);
    plugins = plugins.map((p, idx) =>
      idx === i ? { ...p, name, role: def ? def.role : p.role } : p
    );
  }

  function loadPluginDefaults(i: number) {
    const p = plugins[i];
    const defaults = defaultParamsFor(p.name);
    // Merge: keep existing values, fill in missing defaults
    const existing = Object.fromEntries(p.params.filter(r => r.key).map(r => [r.key, r.value]));
    const merged = { ...defaults, ...existing };
    plugins = plugins.map((pl, idx) =>
      idx === i
        ? { ...pl, params: Object.entries(merged).map(([key, value]) => ({ key, value })) }
        : pl
    );
  }

  function addParam(i: number) {
    plugins = plugins.map((p, idx) =>
      idx === i ? { ...p, params: [...p.params, { key: '', value: '' }] } : p
    );
  }

  function removeParam(pluginIdx: number, paramIdx: number) {
    plugins = plugins.map((p, i) =>
      i === pluginIdx
        ? { ...p, params: p.params.filter((_, j) => j !== paramIdx) }
        : p
    );
  }

  async function save() {
    if (!cfg) return;
    saveError = null;
    saving = true;
    try {
      // Flatten all plugin params into one dict (later plugins override earlier on key collision)
      const parameters: Record<string, string> = {};
      for (const p of plugins) {
        for (const row of p.params) {
          const k = row.key.trim();
          if (k) parameters[k] = row.value;
        }
      }
      const cleanPlugins = plugins.filter(p => p.role.trim() && p.name.trim());
      const universe = editUniverse.split(',').map(s => s.trim()).filter(Boolean);
      cfg = await strategy.update(cfg.id, {
        name: editName,
        description: editDescription || undefined,
        composition: editComposition,
        enabled: editEnabled,
        parameters,
        plugins: cleanPlugins,
        universe,
      });
      plugins = distributeParams(cfg.parameters ?? {}, cfg.plugins.map(p => ({ role: p.role, name: p.name })));
      saved = true;
      setTimeout(() => (saved = false), 2000);
    } catch (e) {
      saveError = String(e);
    } finally {
      saving = false;
    }
  }

  let missingRoles = $derived(() => {
    const assigned = plugins.filter(p => p.name.trim()).map(p => p.role.toLowerCase());
    return (['identifier', 'timer', 'sizer'] as const).filter(r => !assigned.includes(r));
  });
</script>

<div class="p-6 max-w-3xl">
  <div class="flex items-center gap-2 mb-6">
    <a href="/strategies" class="text-sm" style="color: var(--color-text-muted)">← Strategies</a>
  </div>

  {#if loading}
    <div style="color: var(--color-text-muted)">Loading…</div>
  {:else if error}
    <div class="text-sm" style="color: var(--color-accent-red)">{error}</div>
  {:else if cfg}
    <div class="flex items-center justify-between mb-6">
      <h1 class="text-xl font-semibold" style="color: var(--color-text-primary)">Edit Strategy</h1>
      <label class="flex items-center gap-2 text-sm cursor-pointer" style="color: var(--color-text-secondary)">
        <input type="checkbox" bind:checked={editEnabled} />
        Enabled
      </label>
    </div>

    <div class="space-y-5">

      <!-- Basic Info -->
      <div class="rounded-xl p-4 space-y-4" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
        <h2 class="text-xs font-semibold uppercase tracking-wide" style="color: var(--color-text-muted)">Basic Info</h2>
        <div class="grid grid-cols-2 gap-4">
          <div class="col-span-2">
            <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Name</label>
            <input type="text" bind:value={editName} class="w-full rounded-lg px-3 py-2 text-sm"
              style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
          </div>
          <div class="col-span-2">
            <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Description</label>
            <input type="text" bind:value={editDescription} placeholder="Optional" class="w-full rounded-lg px-3 py-2 text-sm"
              style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
          </div>
          <div>
            <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Composition</label>
            <select bind:value={editComposition} class="w-full rounded-lg px-3 py-2 text-sm"
              style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;">
              <option value="pipeline">Pipeline — plugins run in sequence</option>
              <option value="voting">Voting — majority rules</option>
              <option value="ensemble">Ensemble — weighted average</option>
            </select>
          </div>
          <div>
            <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Universe (comma-separated)</label>
            <input type="text" bind:value={editUniverse} placeholder="AAPL, MSFT, GOOGL" class="w-full rounded-lg px-3 py-2 text-sm font-mono"
              style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
          </div>
        </div>
      </div>

      <!-- Plugins -->
      <div class="rounded-xl p-4 space-y-3" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
        <h2 class="text-xs font-semibold uppercase tracking-wide" style="color: var(--color-text-muted)">Plugins</h2>

        {#if missingRoles().length > 0}
          <div class="text-xs rounded-lg px-3 py-2" style="background: rgba(234,179,8,0.1); border: 1px solid rgba(234,179,8,0.3); color: #fbbf24;">
            Missing required roles: {missingRoles().join(', ')} — backtest will fail without them.
          </div>
        {/if}

        <div class="space-y-3">
          {#each plugins as plugin, i}
            {@const availablePlugins = pluginsByRole(plugin.role as PluginRole)}
            {@const def = findPlugin(plugin.name)}
            <div class="rounded-lg p-3 space-y-3" style="background: var(--color-bg-secondary); border: 1px solid var(--color-border)">

              <!-- Role + name row -->
              <div class="flex gap-2 items-center">
                <div class="flex-1">
                  <label class="block text-xs mb-1" style="color: var(--color-text-muted)">Role</label>
                  <select
                    value={plugin.role}
                    onchange={(e) => { plugins = plugins.map((p, idx) => idx === i ? { ...p, role: (e.target as HTMLSelectElement).value, name: '' } : p); }}
                    class="w-full rounded-lg px-3 py-1.5 text-sm"
                    style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;"
                  >
                    {#each PLUGIN_ROLES as role}
                      <option value={role}>{role}</option>
                    {/each}
                  </select>
                </div>
                <div class="flex-1">
                  <label class="block text-xs mb-1" style="color: var(--color-text-muted)">Plugin</label>
                  <select
                    value={plugin.name}
                    onchange={(e) => onPluginNameChange(i, (e.target as HTMLSelectElement).value)}
                    class="w-full rounded-lg px-3 py-1.5 text-sm"
                    style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;"
                  >
                    <option value="">— select plugin —</option>
                    {#each availablePlugins as p}
                      <option value={p.name}>{p.name}</option>
                    {/each}
                  </select>
                </div>
                <div class="self-end">
                  <button onclick={() => removePlugin(i)} class="text-sm px-2 py-1.5 rounded"
                    style="color: var(--color-accent-red); background: var(--color-bg-card); border: 1px solid var(--color-border)">✕</button>
                </div>
              </div>

              {#if def}
                <!-- Plugin description -->
                <div class="text-xs" style="color: var(--color-text-muted)">{def.description}</div>

                <!-- Param hint table -->
                {#if def.params.length > 0}
                  <div class="rounded-lg overflow-hidden text-xs" style="border: 1px solid var(--color-border)">
                    <table class="w-full">
                      <thead>
                        <tr style="border-bottom: 1px solid var(--color-border); background: var(--color-bg-card)">
                          <th class="text-left px-3 py-1.5 font-medium" style="color: var(--color-text-muted)">Parameter</th>
                          <th class="text-left px-3 py-1.5 font-medium" style="color: var(--color-text-muted)">Default</th>
                          <th class="text-left px-3 py-1.5 font-medium" style="color: var(--color-text-muted)">Description</th>
                        </tr>
                      </thead>
                      <tbody>
                        {#each def.params as p}
                          <tr style="border-top: 1px solid var(--color-border)">
                            <td class="px-3 py-1 font-mono" style="color: var(--color-text-primary)">{p.key}</td>
                            <td class="px-3 py-1 font-mono" style="color: var(--color-text-secondary)">{p.default}</td>
                            <td class="px-3 py-1" style="color: var(--color-text-muted)">{p.description}</td>
                          </tr>
                        {/each}
                      </tbody>
                    </table>
                  </div>
                {/if}

                <!-- Inline param editor -->
                {#if plugin.params.length > 0}
                  <div class="space-y-1.5">
                    {#each plugin.params as param, j}
                      <div class="flex gap-2 items-center">
                        <input type="text" bind:value={param.key} placeholder="key" class="rounded-lg px-2 py-1.5 text-xs font-mono flex-1"
                          style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
                        <span class="text-xs" style="color: var(--color-text-muted)">=</span>
                        <input type="text" bind:value={param.value} placeholder="value" class="rounded-lg px-2 py-1.5 text-xs font-mono flex-1"
                          style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;" />
                        <button onclick={() => removeParam(i, j)} class="text-xs px-1.5 py-1 rounded"
                          style="color: var(--color-accent-red); background: var(--color-bg-card); border: 1px solid var(--color-border)">✕</button>
                      </div>
                    {/each}
                  </div>
                {/if}

                <!-- Action buttons -->
                <div class="flex gap-2">
                  <button onclick={() => addParam(i)} class="text-xs px-2 py-1 rounded-lg"
                    style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-secondary)">
                    + Add parameter
                  </button>
                  <button onclick={() => loadPluginDefaults(i)} class="text-xs px-2 py-1 rounded-lg"
                    style="background: var(--color-bg-card); border: 1px solid var(--color-border); color: var(--color-text-secondary)">
                    Load defaults
                  </button>
                </div>
              {/if}

            </div>
          {/each}
        </div>

        <button onclick={addPlugin} class="text-xs px-3 py-1.5 rounded-lg"
          style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-secondary)">
          + Add plugin
        </button>
      </div>

      <!-- Save -->
      {#if saveError}
        <div class="text-sm" style="color: var(--color-accent-red)">{saveError}</div>
      {/if}
      <div class="flex items-center gap-3">
        <button onclick={save} disabled={saving} class="px-5 py-2 rounded-lg text-sm font-medium"
          style="background: var(--color-accent-blue); color: white; opacity: {saving ? 0.6 : 1};">
          {saving ? 'Saving…' : 'Save Changes'}
        </button>
        {#if saved}<span class="text-sm" style="color: var(--color-accent-green)">Saved ✓</span>{/if}
        <a href="/strategies" class="text-sm" style="color: var(--color-text-muted)">Cancel</a>
      </div>

    </div>
  {/if}
</div>
