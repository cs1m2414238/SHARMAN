import type { AuditEvent, BedrockStatus, BridgeStatus, CareExplanation, ChatResult, HomeScenario, HomeState, HomeWorkflow, InboxAction, ProcessingMode, RingStatus, RingPlaygroundDevice, RingPlaygroundSnapshot, Task } from './types';
import { runAlexaCommand } from './mcp';

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...init,
    headers: { 'Content-Type': 'application/json', ...init?.headers },
  });
  if (!response.ok) throw new Error((await response.json().catch(() => null))?.error || `Request failed (${response.status})`);
  return response.json() as Promise<T>;
}

export const getStatus = () => request<BridgeStatus>('/api/status');
export const setProcessingMode = (processingMode: ProcessingMode) => request<{ processingMode: ProcessingMode }>('/api/settings/processing-mode', { method: 'POST', body: JSON.stringify({ processingMode }) });
export const getTasks = () => request<{ tasks: Task[] }>('/api/tasks');
export const createTask = (title: string, dueAt: string | null) => request<{ task: Task; created: boolean }>('/api/tasks', { method: 'POST', body: JSON.stringify({ title, dueAt }), signal: AbortSignal.timeout(15_000) });
export const getActions = (sample = false) => request<{ items: InboxAction[]; permission: string; demoData: boolean }>(`/api/inbox/actions${sample ? '?sample=true' : ''}`);
export const getAudit = () => request<{ events: AuditEvent[] }>('/api/audit');
export const chat = (message: string, surface: 'web' | 'alexa') => surface === 'alexa' ? runAlexaCommand(message) : request<ChatResult>('/api/assistant/chat', { method: 'POST', body: JSON.stringify({ message, surface }) });
export const createReminder = (title: string, remindAt: string, sourceActionId: string) => request('/api/reminders', { method: 'POST', body: JSON.stringify({ title, remindAt, sourceActionId }) });
export const getHome = (signal?: AbortSignal) => request<{ home: HomeState; simulation: true }>('/api/home', { signal });
export const getRingStatus = (signal?: AbortSignal) => request<RingStatus>('/api/ring/status', { signal });
export const getRingPlaygroundDevices = (signal?: AbortSignal) => request<{ devices: RingPlaygroundDevice[] }>('/api/ring/playground/devices', { signal });
export const getRingPlaygroundDevice = (deviceId: string, signal?: AbortSignal) => request<RingPlaygroundSnapshot>(`/api/ring/playground/device?deviceId=${encodeURIComponent(deviceId)}`, { signal });
export const syncRingPlayground = (deviceId: string, signal?: AbortSignal) => request<{ status: string; message?: string }>('/api/ring/playground/sync', { method: 'POST', body: JSON.stringify({ deviceId }), signal });
export const checkRingPermission = (eventId: string, signal?: AbortSignal) => request<HomeWorkflow>('/api/home/check-permission', { method: 'POST', body: JSON.stringify({ eventId }), signal });
export const ingestRingTestEvent = (signal?: AbortSignal) => {
  const now = new Date();
  return request('/api/ring/test-event', { method: 'POST', signal, body: JSON.stringify({
    meta: { version: '1.1', time: now.toISOString(), request_id: crypto.randomUUID(), account_id: 'local-test-account' },
    data: { id: crypto.randomUUID(), type: 'motion_detected', attributes: { source: 'local-test-device', source_type: 'devices', timestamp: now.getTime(), sub_type: 'human' } },
  }) });
};
export const simulateHome = (scenario: HomeScenario, signal?: AbortSignal) => request<HomeWorkflow>('/api/home/simulate', { method: 'POST', body: JSON.stringify({ scenario }), signal });
export const acceptHomeSuggestion = (scenario: Exclude<HomeScenario, 'blocked_unlock'>, signal?: AbortSignal) => request<{ message: string }>('/api/home/accept', { method: 'POST', body: JSON.stringify({ scenario }), signal });
export const resetHomeDemo = (signal?: AbortSignal) => request<{ home: HomeState }>('/api/home/reset', { method: 'POST', signal });
export const getBedrockStatus = (signal?: AbortSignal) => request<BedrockStatus>('/api/aws/status', { signal });
export const explainHomeWithBedrock = (signal?: AbortSignal) => request<CareExplanation>('/api/home/explain', { method: 'POST', signal });
