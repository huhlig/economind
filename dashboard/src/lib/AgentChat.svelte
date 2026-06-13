<script lang="ts">
  import { tick, onMount } from 'svelte';
  import { chat } from '$lib/api.js';
  import type { ChatMessage, ChatPersona, ChatSession } from '$lib/api.js';
  import { pageContext } from '$lib/stores/pageContext.js';

  let draft = $state('');
  let history = $state<ChatMessage[]>([]);
  let sessions = $state<ChatSession[]>([]);
  let sessionId = $state('');
  let personas = $state<ChatPersona[]>([]);
  let personaId = $state('');
  let depth = $state<'basic' | 'detailed' | 'expert'>('detailed');
  let loading = $state(false);
  let loadingSessions = $state(false);
  let loadingPersonas = $state(false);
  let error = $state<string | null>(null);
  let messagesEl = $state<HTMLDivElement | null>(null);

  const suggestions = [
    'Summarize today\'s highest conviction signals.',
    'What portfolio risks need attention?',
    'Compare active strategies over the next month.',
  ];

  onMount(async () => {
    await loadPersonas();
    await loadSessions();
  });

  async function loadSessions() {
    loadingSessions = true;
    try {
      sessions = await chat.sessions();
    } catch {
      sessions = [];
    } finally {
      loadingSessions = false;
    }
  }

  async function loadPersonas() {
    loadingPersonas = true;
    try {
      personas = await chat.personas();
    } catch {
      personas = [];
    } finally {
      loadingPersonas = false;
    }
  }

  async function sendMessage() {
    const message = draft.trim();
    if (!message || loading) return;

    error = null;
    draft = '';
    history = [...history, { role: 'user', content: message }];
    loading = true;
    await scrollToLatest();

    // Prepend page context to the first message of a new conversation
    const ctx = $pageContext;
    const isFirstTurn = history.length === 1 && !sessionId;
    const enrichedMessage = ctx && isFirstTurn ? `${ctx}\n\nUser question: ${message}` : message;

    try {
      const response = await chat.send({
        message: enrichedMessage,
        history: history.slice(0, -1),
        session_id: sessionId || undefined,
        persona_id: personaId || undefined,
        depth,
      });
      history = response.history;
      sessionId = response.session.id;
      personaId = response.session.persona_id ?? personaId;
      depth = response.session.depth ?? depth;
      await loadSessions();
    } catch (e) {
      error = readableError(e);
    } finally {
      loading = false;
      await scrollToLatest();
    }
  }

  function useSuggestion(message: string) {
    draft = message;
  }

  function clearConversation() {
    history = [];
    error = null;
    draft = '';
    sessionId = '';
  }

  async function selectSession(id: string) {
    if (!id) {
      clearConversation();
      return;
    }

    loading = true;
    error = null;
    try {
      const detail = await chat.session(id);
      sessionId = detail.session.id;
      history = detail.history;
      personaId = detail.session.persona_id ?? '';
      depth = detail.session.depth ?? 'detailed';
    } catch (e) {
      error = readableError(e);
    } finally {
      loading = false;
      await scrollToLatest();
    }
  }

  function readableError(e: unknown) {
    const text = e instanceof Error ? e.message : String(e);
    try {
      const parsed = JSON.parse(text);
      return parsed.error ?? text;
    } catch {
      return text;
    }
  }

  async function scrollToLatest() {
    await tick();
    if (messagesEl) messagesEl.scrollTop = messagesEl.scrollHeight;
  }
</script>

<div class="agent-sidebar">
  <div class="agent-chat-header">
    <div>
      <div class="agent-chat-title">Agent</div>
      <div class="agent-chat-subtitle">Low frequency trade analysis</div>
    </div>
  </div>

  <div class="agent-chat-controls">
    <select
      value={sessionId}
      aria-label="Chat session"
      onchange={(e) => selectSession(e.currentTarget.value)}
    >
      <option value="">New chat</option>
      {#each sessions as session}
        <option value={session.id}>{session.title}</option>
      {/each}
    </select>
    <button type="button" class="agent-chat-new-button" title="New chat" aria-label="New chat" onclick={clearConversation}>
      +
    </button>
  </div>

  <div class="agent-chat-controls persona-controls">
    <select bind:value={personaId} aria-label="Persona">
      <option value="">Default agent</option>
      {#each personas as persona}
        <option value={persona.id}>{persona.name || persona.description || persona.id}</option>
      {/each}
    </select>
    <select bind:value={depth} aria-label="Depth">
      <option value="basic">Basic</option>
      <option value="detailed">Detailed</option>
      <option value="expert">Expert</option>
    </select>
  </div>

  <div bind:this={messagesEl} class="agent-chat-messages">
    {#if history.length === 0}
      <div class="agent-chat-empty">
        {#each suggestions as suggestion}
          <button type="button" onclick={() => useSuggestion(suggestion)}>{suggestion}</button>
        {/each}
      </div>
    {:else}
      {#each history as message, index (`${index}-${message.role}`)}
        <div class:user={message.role === 'user'} class="agent-chat-message">
          <div class="agent-chat-role">{message.role === 'user' ? 'You' : 'Agent'}</div>
          <div class="agent-chat-bubble">{message.content}</div>
        </div>
      {/each}
    {/if}

    {#if loading}
      <div class="agent-chat-message">
        <div class="agent-chat-role">Agent</div>
        <div class="agent-chat-bubble muted">Thinking...</div>
      </div>
    {/if}
  </div>

  {#if error}
    <div class="agent-chat-error">{error}</div>
  {/if}

  <form class="agent-chat-composer" onsubmit={(e) => { e.preventDefault(); sendMessage(); }}>
    <textarea
      bind:value={draft}
      rows="3"
      placeholder="Ask about signals, risk, strategies..."
      disabled={loading}
      onkeydown={(e) => {
        if (e.key === 'Enter' && !e.shiftKey) {
          e.preventDefault();
          sendMessage();
        }
      }}
    ></textarea>
    <div class="agent-chat-actions">
      <button type="button" onclick={clearConversation} disabled={loading || history.length === 0}>Clear</button>
      <button type="submit" disabled={loading || draft.trim().length === 0}>Send</button>
    </div>
  </form>
</div>

<style>
  .agent-sidebar {
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .agent-chat-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 18px 18px 14px;
    border-bottom: 1px solid var(--color-border);
  }

  .agent-chat-title {
    color: var(--color-text-primary);
    font-size: 16px;
    font-weight: 700;
  }

  .agent-chat-subtitle {
    margin-top: 2px;
    color: var(--color-text-muted);
    font-size: 12px;
  }

  .agent-chat-controls {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 38px;
    gap: 8px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--color-border);
  }

  .agent-chat-controls.persona-controls {
    grid-template-columns: minmax(0, 1fr) 104px;
    padding-top: 0;
  }

  .agent-chat-controls select,
  .agent-chat-new-button,
  .agent-chat-composer textarea {
    width: 100%;
    border: 1px solid var(--color-border);
    border-radius: 8px;
    background: var(--color-bg-secondary);
    color: var(--color-text-primary);
    outline: none;
  }

  .agent-chat-new-button {
    display: grid;
    height: 34px;
    place-items: center;
    color: var(--color-text-secondary);
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
  }

  .agent-chat-controls select {
    min-width: 0;
    padding: 8px 10px;
    font-size: 12px;
  }

  .agent-chat-messages {
    flex: 1;
    overflow-y: auto;
    padding: 16px 14px;
  }

  .agent-chat-empty {
    display: grid;
    gap: 8px;
  }

  .agent-chat-empty button {
    border: 1px solid var(--color-border);
    border-radius: 8px;
    background: var(--color-bg-card);
    color: var(--color-text-secondary);
    padding: 10px;
    text-align: left;
    font-size: 12px;
    line-height: 1.35;
    cursor: pointer;
  }

  .agent-chat-message {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    margin-bottom: 14px;
  }

  .agent-chat-message.user {
    align-items: flex-end;
  }

  .agent-chat-role {
    color: var(--color-text-muted);
    font-size: 11px;
  }

  .agent-chat-bubble {
    max-width: 92%;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    border: 1px solid var(--color-border);
    border-radius: 8px;
    background: var(--color-bg-card);
    color: var(--color-text-primary);
    padding: 9px 10px;
    font-size: 13px;
    line-height: 1.45;
  }

  .agent-chat-message.user .agent-chat-bubble {
    border-color: rgba(59, 130, 246, 0.55);
    background: rgba(59, 130, 246, 0.18);
  }

  .agent-chat-bubble.muted {
    color: var(--color-text-muted);
  }

  .agent-chat-error {
    margin: 0 14px 10px;
    border: 1px solid rgba(239, 68, 68, 0.4);
    border-radius: 8px;
    background: rgba(239, 68, 68, 0.12);
    color: #fecaca;
    padding: 9px 10px;
    font-size: 12px;
    line-height: 1.4;
  }

  .agent-chat-composer {
    padding: 14px;
    border-top: 1px solid var(--color-border);
    background: var(--color-bg-sidebar);
  }

  .agent-chat-composer textarea {
    display: block;
    min-height: 84px;
    resize: none;
    padding: 10px;
    font-size: 13px;
    line-height: 1.4;
  }

  .agent-chat-actions {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    margin-top: 10px;
  }

  .agent-chat-actions button {
    min-width: 72px;
    border: 1px solid var(--color-border);
    border-radius: 8px;
    background: var(--color-bg-card);
    color: var(--color-text-secondary);
    padding: 8px 12px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .agent-chat-actions button[type='submit'] {
    border-color: var(--color-accent-blue);
    background: var(--color-accent-blue);
    color: white;
  }

  .agent-chat-actions button:disabled,
  .agent-chat-composer textarea:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }
</style>
