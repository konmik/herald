export const DEFAULT_SUMMARY_PROMPT: string
export const SUMMARY_PROMPT_MAX_LENGTH: number
export function isValidSummaryPrompt(value: unknown): boolean
export function readAnnouncementProfile(directory?: string): { prompt: string; characterID?: string }
