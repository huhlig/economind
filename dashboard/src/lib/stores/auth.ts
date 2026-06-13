import { writable, derived } from 'svelte/store';
import { resetWsClient } from '$lib/ws.js';

function createAuthStore() {
  const stored = typeof localStorage !== 'undefined' ? (localStorage.getItem('api_key') ?? '') : '';
  const { subscribe, set } = writable<string>(stored);

  return {
    subscribe,
    login(key: string) {
      localStorage.setItem('api_key', key);
      resetWsClient();
      set(key);
    },
    logout() {
      localStorage.removeItem('api_key');
      resetWsClient();
      set('');
    },
  };
}

export const apiKey = createAuthStore();
export const isAuthenticated = derived(apiKey, ($key) => $key.length > 0);
