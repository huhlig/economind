<script lang="ts">
  import '../app.css';
  import { page } from '$app/stores';
  import { isAuthenticated, apiKey } from '$lib/stores/auth.js';
  import { eventLog } from '$lib/stores/events.js';
  import { getWsClient } from '$lib/ws.js';
  import AgentChat from '$lib/AgentChat.svelte';
  import { onMount } from 'svelte';

  let { children } = $props();

  const nav = [
    { href: '/',            label: 'Overview',         icon: '⬡' },
    { href: '/signals',     label: 'Signals',          icon: '⚡' },
    { href: '/portfolio',   label: 'Portfolio',        icon: '◈' },
    { href: '/strategies',  label: 'Strategies',       icon: '⟳' },
    { href: '/backtest',    label: 'Backtest',         icon: '◷' },
    { href: '/data',        label: 'Data',             icon: '≋' },
    { href: '/settings',    label: 'Settings',         icon: '⚙' },
  ];

  const SIDEBAR_MIN = 220;
  const SIDEBAR_MAX = 600;
  const STORAGE_KEY = 'agent-sidebar-width';

  let sidebarWidth = $state(320);
  let dragging = $state(false);

  onMount(() => {
    const saved = parseInt(localStorage.getItem(STORAGE_KEY) ?? '', 10);
    if (saved >= SIDEBAR_MIN && saved <= SIDEBAR_MAX) sidebarWidth = saved;

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

  function startDrag(e: MouseEvent) {
    e.preventDefault();
    dragging = true;

    function onMove(e: MouseEvent) {
      // Sidebar is on the right; width = distance from right edge of viewport
      const newWidth = Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, window.innerWidth - e.clientX));
      sidebarWidth = newWidth;
    }

    function onUp() {
      dragging = false;
      localStorage.setItem(STORAGE_KEY, String(sidebarWidth));
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    }

    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }
</script>

{#if !$isAuthenticated}
  <div class="flex h-screen items-center justify-center" style="background: var(--color-bg-primary)">
    <div class="w-80 rounded-xl p-8" style="background: var(--color-bg-card); border: 1px solid var(--color-border)">
      <div class="mb-6 text-center">
        <div class="text-2xl font-bold" style="color: var(--color-text-primary)">Economind</div>
        <div class="text-sm mt-1" style="color: var(--color-text-muted)">Low Frequency Trading Platform</div>
      </div>
      <form onsubmit={(e) => { e.preventDefault(); const fd = new FormData(e.currentTarget); apiKey.login(String(fd.get('key') ?? '')); }}>
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
  <div class="flex h-screen overflow-hidden" style="cursor: {dragging ? 'col-resize' : 'auto'}; user-select: {dragging ? 'none' : 'auto'}">
    <!-- Left nav -->
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

    <!-- Main content -->
    <main class="flex-1 min-w-0 overflow-y-auto" style="background: var(--color-bg-primary)">
      {@render children()}
    </main>

    <!-- Drag handle -->
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize agent sidebar"
      onmousedown={startDrag}
      style="
        width: 5px;
        flex-shrink: 0;
        cursor: col-resize;
        background: {dragging ? 'var(--color-accent-blue)' : 'var(--color-border)'};
        transition: background 0.15s;
      "
    ></div>

    <!-- Agent sidebar -->
    <aside
      style="
        width: {sidebarWidth}px;
        flex-shrink: 0;
        display: flex;
        flex-direction: column;
        overflow: hidden;
        background: var(--color-bg-sidebar);
        border-left: 1px solid var(--color-border);
      "
    >
      <AgentChat />
    </aside>
  </div>
{/if}
