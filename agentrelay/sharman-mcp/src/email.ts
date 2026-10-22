import type { SharmanStore } from './store.js';
import type { NativePersonalBridge } from './native.js';
export type EmailRequest = { kind: 'status' | 'recent' | 'unread' | 'actions'; sample: boolean; mode: 'important' | 'needsAction' | 'internship' };
export type MailboxReader = Pick<SharmanStore, 'mailboxStatus' | 'actionItems' | 'refreshActions'> & { native: Pick<NativePersonalBridge, 'gmailMessages'> };

export async function readMailbox(store: MailboxReader, kind: EmailRequest['kind'], mode: string, sample: boolean) {
  const mailbox = await store.mailboxStatus();
  if (sample) return { mailbox, dataSource: 'sample', demoData: true, items: await store.actionItems(undefined, true), messages: [], scanned: null, permission: 'READ_ONLY', error: null };
  if (!mailbox.connected || mailbox.needsReconnect) return { mailbox, dataSource: 'unavailable', demoData: false, items: [], messages: [], scanned: null, permission: 'READ_ONLY', error: null };
  if (kind === 'status') return { mailbox, dataSource: 'native_gmail', demoData: false, items: [], messages: [], scanned: null, permission: 'READ_ONLY', error: null };
  try {
    if (kind === 'recent' || kind === 'unread') return { ...await store.native.gmailMessages(kind === 'unread'), mailbox, dataSource: 'native_gmail', demoData: false, items: [], permission: 'READ_ONLY', error: null };
    const result = await store.refreshActions(mode);
    return { ...result, messages: [], mailbox, dataSource: 'native_gmail', demoData: false, permission: 'READ_ONLY', error: null };
  } catch (error) {
    const needsReconnect = /authorization|invalid_grant|revoked/i.test(error instanceof Error ? error.message : '');
    return { mailbox: { ...mailbox, needsReconnect }, dataSource: 'unavailable', demoData: false, items: [], messages: [], scanned: null, permission: 'READ_ONLY', error: needsReconnect ? 'GMAIL_RECONNECT_REQUIRED' : 'GMAIL_READ_FAILED', recoverable: true };
  }
}

export function requestsSampleData(text: string): boolean {
  return !/\b(no|not|without|don't|do not)\b/i.test(text)
    && (/\b(show|preview|explore|use|using|load|view|list|demonstrate)\b.*\b(sample|demo|example)\b/i.test(text) || /^(sample|demo|example)\b/i.test(text));
}

export function classifyEmailRequest(message: string): EmailRequest | null {
  const text = message.toLowerCase();
  if (!/\b(e-?mails?|mails?|gmails?|inbox)\b/.test(text)) return null;
  const sample = requestsSampleData(text);
  const mode = /internship|assessment|\boa\b|interview|career/.test(text) ? 'internship' : /important|starred/.test(text) ? 'important' : 'needsAction';
  const kind = /connection|connected|authorization|diagnostic/.test(text) ? 'status'
    : /unread/.test(text) ? 'unread'
      : /recent|latest|newest/.test(text) && !/important|action|internship|assessment|interview/.test(text) ? 'recent'
        : /action|important|internship|assessment|interview|career|\boa\b/.test(text) || sample ? 'actions' : 'recent';
  return { kind, sample, mode };
}

export const mailboxUnavailable = (needsReconnect = false) => needsReconnect
  ? 'Gmail authorization needs reconnecting. Reconnect Gmail in the private desktop Action Inbox. No sample messages were substituted.'
  : 'Gmail is not connected to this console. Recent and unread emails are available through the private local SHARMAN app after connecting Gmail. This public demo cannot access your personal inbox; no sample messages were substituted.';
