import { writable } from 'svelte/store';

/** Context string that pages can set so the Agent knows what's being displayed. */
export const pageContext = writable<string>('');
