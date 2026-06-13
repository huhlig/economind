import { writable } from 'svelte/store';
import type { ServerEvent } from '$lib/ws.js';

export interface EventLogEntry {
  id: number;
  ts: Date;
  event: ServerEvent;
}

let _seq = 0;

function createEventLog() {
  const { subscribe, update } = writable<EventLogEntry[]>([]);
  return {
    subscribe,
    push(event: ServerEvent) {
      update((log) => {
        const next = [{ id: _seq++, ts: new Date(), event }, ...log];
        return next.slice(0, 200); // keep last 200 events
      });
    },
    clear() {
      update(() => []);
    },
  };
}

export const eventLog = createEventLog();
