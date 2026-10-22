import { spawn } from 'node:child_process';
import path from 'node:path';
import { readFile, stat } from 'node:fs/promises';

export type MailboxStatus = { connected: boolean; needsReconnect: boolean; source: 'native_gmail' | 'unavailable' };
export type MailMessage = { id: string; threadId: string; subject: string; sender: string; snippet: string; receivedAt: string };
type NativeEmailAction = { id: string; messageId: string; title: string; evidence: string; sender: string; deadline: string | null; confidence: number; actionType: string; state: string; reminderCreated: boolean };
export type GmailActions = { items: NativeEmailAction[]; scan: { scanned: number; matched: number } | null };

type NativeTask = { id: string; title: string; status: string; dueAt: string | null; createdAt: string };
type NativeReminder = { id: string; title: string; remindAt: string; status: string; createdAt: string; firedAt: string | null };
type NativeSnapshot = { tasks: NativeTask[]; reminders: NativeReminder[] };

export class NativePersonalBridge {
  readonly executable = process.env.SHARMAN_NATIVE_EXECUTABLE || null;
  get enabled(): boolean { return this.executable !== null; }

  private compatibleFile: string | null = null;

  private async call<T>(request: Record<string, unknown>): Promise<T> {
    const executable = this.executable;
    if (!executable || !path.isAbsolute(executable)) return Promise.reject(new Error('Configure an absolute SHARMAN_NATIVE_EXECUTABLE path.'));
    // Never probe an old GUI executable with an unknown CLI flag: older builds
    // open a new tray/window instead. Check a compiled protocol marker first.
    const metadata = await stat(executable);
    const identity = `${metadata.size}:${metadata.mtimeMs}`;
    if (this.compatibleFile !== identity) {
      if (metadata.size > 128 * 1024 * 1024 || !(await readFile(executable)).includes(Buffer.from('SHARMAN_PERSONAL_BRIDGE_V2'))) {
        throw new Error('This desktop executable lacks the current native bridge. Update SHARMAN and configure the new executable; no GUI was launched.');
      }
      this.compatibleFile = identity;
    }
    return new Promise((resolve, reject) => {
      const child = spawn(executable, ['--personal-bridge'], { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
      let output = '';
      let finished = false;
      const finish = (error?: Error, value?: T) => {
        if (finished) return;
        finished = true;
        clearTimeout(timer);
        if (error) reject(error); else resolve(value as T);
      };
      const timer = setTimeout(() => {
        child.kill();
        finish(new Error('Native SHARMAN did not confirm the tool result in time. No write was retried; check the desktop app before repeating it.'));
      }, String(request.tool).startsWith('gmail_') ? 30_000 : 10_000);
      child.on('error', () => finish(new Error('Native SHARMAN could not start. Check the configured executable path.')));
      child.stdin.on('error', () => finish(new Error('Native SHARMAN could not receive the tool request.')));
      child.stdout.on('data', (chunk: Buffer) => {
        output += chunk.toString();
        if (output.length > 1_048_576) { child.kill(); finish(new Error('Native SHARMAN exceeded the response size limit.')); }
      });
      child.stderr.resume();
      child.on('close', (code) => {
        if (code !== 0) { finish(new Error('Native SHARMAN exited before confirming the tool result.')); return; }
        try {
          const result = JSON.parse(output) as { ok: boolean; result?: T; error?: string };
          if (!result.ok || result.result === undefined) finish(new Error(result.error || 'Native SHARMAN rejected the tool request.'));
          else finish(undefined, result.result);
        } catch { finish(new Error('Native SHARMAN returned an invalid tool response. Rebuild the desktop executable.')); }
      });
      child.stdin.end(JSON.stringify(request));
    });
  }

  snapshot(): Promise<NativeSnapshot> { return this.call({ tool: 'snapshot' }); }
  gmailStatus(): Promise<MailboxStatus> { return this.call({ tool: 'gmail_status' }); }
  gmailMessages(unread: boolean): Promise<{ query: string; scanned: number; messages: MailMessage[] }> { return this.call({ tool: 'gmail_messages', unread }); }
  gmailActions(refresh = false, mode = 'needsAction'): Promise<GmailActions> { return this.call({ tool: 'gmail_actions', refresh, mode }); }
  createTask(title: string, dueAt: string | null): Promise<{ task: NativeTask; created: boolean }> { return this.call({ tool: 'create_task', title, dueAt }); }
  createReminder(title: string, remindAt: string): Promise<{ reminder: NativeReminder; created: boolean }> { return this.call({ tool: 'create_reminder', title, remindAt }); }
}
