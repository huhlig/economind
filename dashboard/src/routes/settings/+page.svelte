<script lang="ts">
  import { apiKey } from '$lib/stores/auth.js';

  let newKey = $state('');
  let saved = $state(false);

  function saveKey() {
    if (newKey.trim()) {
      apiKey.login(newKey.trim());
      newKey = '';
      saved = true;
      setTimeout(() => (saved = false), 2000);
    }
  }
</script>

<div class="p-6 max-w-lg">
  <h1 class="text-xl font-semibold mb-6" style="color: var(--color-text-primary)">Settings</h1>

  <div class="rounded-xl p-5 space-y-5" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
    <h2 class="text-sm font-medium" style="color: var(--color-text-primary)">API Key</h2>

    <div>
      <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Current key</label>
      <div class="rounded-lg px-3 py-2 text-sm font-mono" style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-muted)">
        {$apiKey ? '••••••••' + $apiKey.slice(-4) : '(not set)'}
      </div>
    </div>

    <div>
      <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">Replace key</label>
      <input
        type="password"
        bind:value={newKey}
        placeholder="New API key"
        class="w-full rounded-lg px-3 py-2 text-sm mb-3"
        style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;"
      />
      <div class="flex items-center gap-3">
        <button
          onclick={saveKey}
          disabled={!newKey.trim()}
          class="px-5 py-2 rounded-lg text-sm font-medium"
          style="background: var(--color-accent-blue); color: white; opacity: {!newKey.trim() ? 0.5 : 1};"
        >
          Save Key
        </button>
        {#if saved}
          <span class="text-sm" style="color: var(--color-accent-green)">Saved ✓</span>
        {/if}
      </div>
    </div>
  </div>

  <div class="rounded-xl p-5 mt-5" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
    <h2 class="text-sm font-medium mb-4" style="color: var(--color-text-primary)">Session</h2>
    <button
      onclick={() => apiKey.logout()}
      class="px-5 py-2 rounded-lg text-sm font-medium"
      style="background: #7f1d1d; color: #fca5a5; border: 1px solid #991b1b;"
    >
      Sign Out
    </button>
    <p class="text-xs mt-2" style="color: var(--color-text-muted)">
      Clears your API key from local storage and disconnects the WebSocket.
    </p>
  </div>

  <div class="rounded-xl p-5 mt-5" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
    <h2 class="text-sm font-medium mb-2" style="color: var(--color-text-primary)">About</h2>
    <p class="text-xs" style="color: var(--color-text-muted)">
      Economind — Low Frequency Trading Analysis Platform<br />
      SvelteKit dashboard · Rust/Axum backend · Lightweight Charts
    </p>
  </div>
</div>
