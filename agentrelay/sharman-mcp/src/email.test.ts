import assert from 'node:assert/strict';
import test from 'node:test';
import { classifyEmailRequest, requestsSampleData, mailboxUnavailable, readMailbox, type MailboxReader } from './email.js';
import { SharmanStore } from './store.js';

test('recent, unread and actionable email requests stay separate', () => {
  for (const phrase of ['recent emails', 'show latest gmails', 'newest mail', 'check my inbox']) assert.equal(classifyEmailRequest(phrase)?.kind, 'recent');
  for (const phrase of ['Unread mails', 'all unread gmails', 'show my unread inbox', 'recent unread emails']) assert.equal(classifyEmailRequest(phrase)?.kind, 'unread');
  for (const phrase of ['Show my important emails', 'What emails need action today?', 'Find internship and OA emails']) assert.equal(classifyEmailRequest(phrase)?.kind, 'actions');
  assert.equal(classifyEmailRequest('Gmail connection status')?.kind, 'status');
  assert.equal(classifyEmailRequest('play music'), null);
});

test('samples require an explicit positive request, not a negative mention', () => {
  for (const phrase of ['Preview sample inbox', 'show demo emails', 'plan my day using sample data']) assert.equal(requestsSampleData(phrase), true);
  for (const phrase of ['recent emails', 'show my real emails not demo', "don't show sample emails", 'emails without examples']) assert.equal(requestsSampleData(phrase), false);
  assert.match(mailboxUnavailable(), /not connected/);
  assert.match(mailboxUnavailable(true), /needs reconnecting/);
});

test('unconfigured mailbox is unavailable and never silently falls back to seeded samples', async () => {
  const store = new SharmanStore('this-directory-must-not-be-read-for-disconnected-mail');
  assert.equal((await store.mailboxStatus()).connected, false);
  assert.deepEqual(await store.actionItems(), []);
});

function gateway(): MailboxReader {
  return {
    mailboxStatus: async () => ({ connected: true, needsReconnect: false, source: 'native_gmail' }),
    actionItems: async () => { throw new Error('A metadata listing must not invoke action extraction or sample fallback'); },
    refreshActions: async () => ({ items: [], scanned: 9 }),
    native: { gmailMessages: async (unread) => ({ query: unread ? 'is:unread' : 'in:inbox newer_than:14d', scanned: 1, messages: [{ id: 'fixture-only', threadId: 'abcd', subject: 'Ordinary conversation, no action keywords', sender: 'Fixture sender', snippet: 'Fixture preview', receivedAt: '2026-10-20T08:00:00Z' }] }) },
  };
}

test('private recent and unread listings return actual metadata even without action signals', async () => {
  for (const kind of ['recent', 'unread'] as const) {
    const result = await readMailbox(gateway(), kind, 'needsAction', false);
    assert.equal(result.dataSource, 'native_gmail');
    assert.equal(result.messages.length, 1);
    assert.equal(result.scanned, 1);
    assert.deepEqual(result.items, []);
  }
});

test('zero messages is distinct from a disconnected or revoked mailbox', async () => {
  const store = gateway();
  store.native.gmailMessages = async () => ({ query: 'is:unread', scanned: 0, messages: [] });
  const empty = await readMailbox(store, 'unread', 'needsAction', false);
  assert.equal(empty.dataSource, 'native_gmail');
  assert.equal(empty.mailbox.connected, true);
  assert.equal(empty.scanned, 0);
  store.mailboxStatus = async () => ({ connected: false, needsReconnect: false, source: 'unavailable' });
  const disconnected = await readMailbox(store, 'recent', 'needsAction', false);
  assert.equal(disconnected.dataSource, 'unavailable');
  assert.equal(disconnected.scanned, null);
  store.mailboxStatus = async () => ({ connected: true, needsReconnect: true, source: 'native_gmail' });
  const revoked = await readMailbox(store, 'recent', 'needsAction', false);
  assert.equal(revoked.dataSource, 'unavailable');
  assert.equal(revoked.mailbox.needsReconnect, true);
});

test('provider errors are recoverable, sanitized, and never replaced by samples', async () => {
  for (const reason of ['network unreachable [private provider payload]', 'authorization needs reconnecting']) {
    const store = gateway();
    store.native.gmailMessages = async () => { throw new Error(reason); };
    const result = await readMailbox(store, 'recent', 'needsAction', false);
    assert.equal(result.dataSource, 'unavailable');
    assert.deepEqual(result.messages, []);
    assert.equal(result.demoData, false);
    assert.equal(JSON.stringify(result).includes('[private provider payload]'), false);
    assert.ok(result.error);
  }
});
