<script lang="ts">
  import '../app.css';
  import { page } from '$app/stores';
  import { isAuthenticated, apiKey } from '$lib/stores/auth.js';
  import { eventLog } from '$lib/stores/events.js';
  import { getWsClient } from '$lib/ws.js';
  import { onMount } from 'svelte';

  let { children } = $props();

  const nav = [
    { href: '/',            label: 'Overview',         icon: '⬡' },
    { href: '/signals',     label: 'Signals',          icon: '⚡' },
    { href: '/portfolio',   label: 'Portfolio',        icon: '◈' },
    { href: '/strategies',  label: 'Strategies',       icon: '⟳' },
    { href: '/backtest',    label: 'Backtest',         icon: '◷' },
    { href: '/data',        label: 'Data Explorer',    icon: '≋' },
    { href: '/settings',    label: 'Settings',         icon: '⚙' },
  ];

  onMount(() => {
    if ($isAuthenticated) {
      const ws = getWsClient();
      ws.connect();
      const off = ws.on((evt) => eventLog.push(evt));
      return () => {
        off();
        ws.disconnect();
      };
    }
  });
</script>

{#if !$isAuthenticated}
  <div class="flex h-screen items-center justify-center" style="background: var(--color-bg-primary)">
    <div class="w-80 rounded-xl p-8" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
      <div class="mb-6 text-center">
        <div class="text-2xl font-bold" style="color: var(--color-text-primary)">Economind</div>
        <div class="text-sm mt-1" style="color: var(--color-text-muted)">Low Frequency Trading Platform</div>
      </div>
      <form onsubmit={(e) => { e.preventDefault(); const fd = new FormData(e.currentTarget); apiKey.login(fd.get('key')); }}>
        <label class="block text-xs mb-1" style="color: var(--color-text-secondary)">API Key</label>
        <input
          type="password"
          name="key"
          placeholder="Bearer token"
          class="w-full rounded-lg px-3 py-2 text-sm mb-4"
          style="background: var(--color-bg-secondary); border: 1px solid var(--color-border); color: var(--color-text-primary); outline: none;"
          required
        />
        <button
          type="submit"
          class="w-full rounded-lg py-2 text-sm font-medium"
          style="background: var(--color-accent-blue); color: white;"
        >Connect</button>
      </form>
    </div>
  </div>
{:else}
  <div class="flex h-screen overflow-hidden">
    <nav class="flex flex-col w-52 shrink-0 overflow-y-auto"
         style="background: var(--color-bg-sidebar); border-right: 1px solid var(--color-border);">
      <div class="px-4 py-5">
        <span class="text-lg font-bold" style="color: var(--color-text-primary)">Economind</span>
      </div>
      <ul class="flex-1 px-2 space-y-0.5">
        {#each nav as item}
          {@const active = $page.url.pathname === item.href || ($page.url.pathname.startsWith(item.href) && item.href !== '/')}
          <li>
            <a
              href={item.href}
              class="flex items-center gap-3 px-3 py-2 rounded-lg text-sm transition-colors"
              style={active
                ? 'background: var(--color-bg-card); color: var(--color-text-primary); font-weight: 500;'
                : 'color: var(--color-text-secondary);'}
            >
              <span class="text-base">{item.icon}</span>
              {item.label}
            </a>
          </li>
        {/each}
      </ul>
      <div class="p-4 border-t" style="border-color: var(--color-border)">
        <button
          onclick={() => apiKey.logout()}
          class="w-full text-left text-xs"
          style="color: var(--color-text-muted)"
        >Sign out</button>
      </div>
    </nav>

    <main class="flex-1 overflow-y-auto" style="background: var(--color-bg-primary)">
      {@render children()}
    </main>
  </div>
{/if}
