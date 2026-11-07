export type Status = 'RECORDED' | 'TRANSCRIBING' | 'TRANSCRIBED' | 'SUMMARIZING' | 'READY' | 'TRANSCRIPTION_FAILED' | 'SUMMARY_FAILED';
export interface ActionItem { id: string; task: string; owner: string | null; deadline: string | null; completed: boolean }
export interface Meeting { id: string; generatedTitle: string; customTitle: string | null; createdAt: string; startedAt: string; endedAt: string; durationSeconds: number; audioPath: string; transcript: string | null; summary: string | null; decisions: string[]; actionItems: ActionItem[]; importantDetails: string[]; status: Status; error: string | null; transcriptionModel: string; summaryModel: string }
export interface Device { id: string; name: string }
export interface Settings { theme: 'light' | 'dark' | 'system'; language: string; launchAtLogin: boolean; minimizeToTray: boolean; microphoneId: string; systemAudioId: string; recordingPath: string; transcriptionModel: string; summaryModel: string; summaryLanguage: string; shortcut: string }
export interface Snapshot { meetings: Meeting[]; settings: Settings; keyConnected: boolean; recordingStartedAt: string | null; microphones: Device[]; outputs: Device[]; microphoneReady: boolean; systemAudioReady: boolean }
export interface TimedSegment { start: number; end: number; speaker: string; text: string }
export interface Collection { name: string; color: string; count: number }
export interface MeetingExtra { collection: string; bookmarks: number[]; speakerNames: Record<string,string>; segments: TimedSegment[]; detectedMode: string | null; translation: string | null }
export interface ExtrasSnapshot { collections: Collection[]; meetings: Record<string, MeetingExtra> }
export interface Analysis { mode: string; title: string; summary: string; decisions: string[]; details: string[]; actionItems: {task: string;owner: string | null;deadline: string | null}[] }
export interface AskResponse { answer: string; quote: string }
export interface SearchResult { meetingId: string; title: string; startedAt: string; source: string; snippet: string }
export const titleOf = (meeting: Meeting) => meeting.customTitle?.trim() || meeting.generatedTitle.trim() || 'Untitled meeting';
export const formatDate = (value: string, options: Intl.DateTimeFormatOptions = { month: 'short', day: 'numeric', year: 'numeric' }) => new Intl.DateTimeFormat(uiLocale(), options).format(new Date(value));
export const formatDuration = (seconds: number) => { const minutes = Math.round(seconds / 60); return uiLocale() === 'ru-RU' ? (minutes < 60 ? `${minutes} мин` : `${Math.floor(minutes / 60)} ч ${minutes % 60} мин`) : (minutes < 60 ? `${minutes} min` : `${Math.floor(minutes / 60)} h ${minutes % 60} min`); };
import { uiLocale } from './i18n';
