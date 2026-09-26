import { invoke } from '@tauri-apps/api/core';
import type { Meeting, SearchResult, Settings, Snapshot } from './types';
export const api = {
  snapshot: () => invoke<Snapshot>('get_snapshot'),
  start: () => invoke<void>('start_recording'), stop: () => invoke<Meeting>('stop_recording'),
  rename: (id: string, title: string) => invoke<void>('rename_meeting', { id, title }),
  remove: (id: string) => invoke<void>('delete_meeting', { id }),
  toggleAction: (id: string, completed: boolean) => invoke<void>('toggle_action', { id, completed }),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  saveKey: (key: string) => invoke<void>('save_api_key', { key }),
  retry: (id: string) => invoke<void>('retry_processing', { id }),
  search: (query: string, source: string) => invoke<SearchResult[]>('search_meetings', { query, source }),
  chooseStorage: () => invoke<string | null>('choose_storage'),
  openStorage: () => invoke<void>('open_storage'),
  openAudio: (id: string) => invoke<void>('open_audio', { id }),
};
