import { AnimatePresence, motion } from 'framer-motion';
import {
  Activity, BellRing, Bot, Check, ChevronRight, CircleUserRound, Clock3, Cloud, Command,
  ExternalLink, House, Inbox, ListTodo, LockKeyhole, Mail, Menu, MessageSquareText,
  Mic, Plus, Radio, Send, Server, ShieldCheck, Sparkles, Unplug, X,
} from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { chat, createReminder, createTask, getActions, getAudit, getStatus, getTasks, setProcessingMode } from './api';
import HomeView from './HomeView';
import { connectAlexaMcp, type McpConnection } from './mcp';
import type { AuditEvent, BridgeStatus, ChatResult, InboxAction, ProcessingMode, Task } from './types';

type View = 'assistant' | 'home' | 'inbox' | 'tasks' | 'activity' | 'connections' | 'alexa';
type Turn = { id: string; role: 'user' | 'assistant'; text: string; trace?: ChatResult };

const quickPrompts = ['What do I need to do today?', 'What emails need action today?', 'Show my open tasks'];

const formatDate = (value: string | null) => {
  if (!value) return 'No deadline';
  const date = new Date(value);
  const tomorrow = new Date();
  tomorrow.setDate(tomorrow.getDate() + 1);
  const prefix = date.toDateString() === tomorrow.toDateString() ? 'Tomorrow · ' : '';
  return `${prefix}${new Intl.DateTimeFormat('en-IN', { day: 'numeric', month: 'short', hour: 'numeric', minute: '2-digit' }).format(date)}`;
};

const relativeTime = (value: string) => {
  const minutes = Math.max(0, Math.round((Date.now() - new Date(value).getTime()) / 60_000));
  if (minutes < 1) return 'now';
  if (minutes < 60) return `${minutes}m ago`;
  return `${Math.round(minutes / 60)}h ago`;
};

function Brand() {
  return <div className="brand"><div className="brand-orbit"><span>S</span><i /><b /></div><div><strong>SHARMAN</strong><small>Personal AI supervisor</small></div></div>;
}

function SideNav({ view, setView, actions, mobile, close }: { view: View; setView: (view: View) => void; actions: number; mobile: boolean; close: () => void }) {
  const links: Array<[View, string, typeof Bot, number?]> = [
    ['assistant', 'Assistant', MessageSquareText], ['home', 'Home', House], ['inbox', 'Action Inbox', Inbox, actions], ['tasks', 'Tasks', ListTodo],
    ['activity', 'Activity', Activity], ['connections', 'Connections', Radio], ['alexa', 'Alexa+', Sparkles],
  ];
  return <aside className={`sidebar ${mobile ? 'mobile-open' : ''}`}>
    <div className="sidebar-head"><Brand />{mobile && <button className="icon-button" onClick={close} aria-label="Close menu"><X /></button>}</div>
    <nav>{links.map(([id, label, Icon, badge]) => <button className={view === id ? 'active' : ''} onClick={() => { setView(id); close(); }} key={id}><Icon /><span>{label}</span>{Boolean(badge) && <em>{badge}</em>}</button>)}</nav>
    <div className="sidebar-safety"><ShieldCheck /><div><strong>Local control</strong><small>Tools need permission</small></div></div>
    <div className="profile"><span>S</span><div><strong>SHARMAN workspace</strong><small>Permission-aware assistant</small></div><CircleUserRound /></div>
  </aside>;
}

function Shell({ children, view, setView, status, menuOpen, setMenuOpen }: { children: React.ReactNode; view: View; setView: (view: View) => void; status: BridgeStatus | null; menuOpen: boolean; setMenuOpen: (open: boolean) => void }) {
  return <div className="app-shell">
    <SideNav view={view} setView={setView} actions={status?.counts.actions || 0} mobile={menuOpen} close={() => setMenuOpen(false)} />
    {menuOpen && <button className="scrim" onClick={() => setMenuOpen(false)} aria-label="Close menu" />}
    <main className="main-surface">
      <header className="topbar"><button className="menu-button" onClick={() => setMenuOpen(true)} aria-label="Open menu"><Menu /></button><div className="mobile-brand"><Brand /></div><div className="runtime-pill" title="This console uses deterministic rules and controlled tools. No local LLM is connected."><span /> RULE-BASED RUNTIME</div><div className={`bridge-pill ${status ? 'online' : ''}`}><Server />{status ? 'MCP bridge online' : 'Bridge unavailable'}</div></header>
      {children}
    </main>
  </div>;
}

function InboxCard({ item, onRemind, compact = false }: { item: InboxAction; onRemind: (item: InboxAction) => void; compact?: boolean }) {
  return <motion.article layout className={`inbox-card ${compact ? 'compact' : ''}`} initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }}>
    <div className="sender-icon"><Mail /></div>
    <div className="inbox-body"><div className="inbox-meta"><span>{item.sender}</span><em>{Math.round(item.confidence * 100)}% confidence</em></div><h3>{item.subject}</h3><p>{item.summary}</p><div className="deadline"><Clock3 />{formatDate(item.deadline)}</div></div>
    <div className="card-actions">{item.sample ? <span className="sample-email-label">Sample email · not your inbox</span> : item.deadline && <button className="primary-small" onClick={() => onRemind(item)}><BellRing /> Remind me</button>}</div>
  </motion.article>;
}

function ExecutionTrace({ trace }: { trace: ChatResult }) {
  const delegation = trace.result.delegation;
  return <div className="trace-card">
    <div className="trace-head"><Command /><strong>{delegation ? `Temporary worker · ${delegation.name}` : 'Agent execution'}</strong><span>{trace.permission === 'READ_ONLY' ? <><LockKeyhole /> Read only</> : trace.permission}</span></div>
    <div className="trace-flow"><div><small>UNDERSTAND</small><strong>{trace.intent.replaceAll('_', ' ')}</strong></div><ChevronRight />
      <div><small>PLAN</small><strong>{delegation ? `${delegation.stepsUsed}/${delegation.maxSteps} step budget` : `${trace.plan.length} controlled steps`}</strong></div><ChevronRight />
      <div><small>TOOL</small><strong>{trace.tool || 'No tool needed'}</strong></div><ChevronRight />
      <div className="success"><small>RESULT</small><strong>{delegation ? 'Agent terminated' : trace.result.dataSource === 'sample' ? 'Sample preview' : trace.result.error ? 'Read failed' : trace.result.mailbox && !trace.result.mailbox.connected ? 'Gmail not connected' : trace.result.messages ? `${trace.result.messages.length} messages` : trace.result.actionable !== undefined ? `${trace.result.actionable} actionable` : `${trace.result.tasks?.length || 0} tasks`}</strong></div></div>
    {trace.result.dataSource === 'sample' && <div className="mailbox-notice">Sample data only · no personal Gmail search was performed.</div>}
    {delegation && <div className="delegation-detail" style={{ padding: '18px', borderTop: '1px solid rgba(255,255,255,0.08)', display: 'grid', gap: 14 }}>
      <div className="delegation-receipt"><span><Check /> Complex request delegated</span><strong>{delegation.name} created</strong><small>Supervisor-confirmed · {delegation.status} · {delegation.stepsUsed}/{delegation.maxSteps} steps · {delegation.timeoutMs / 1000}s limit</small></div>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, fontSize: 12 }}>
        <span><strong>Goal:</strong> {delegation.goal}</span>
        <span>· <strong>Ceiling:</strong> {delegation.permissionCeiling}</span>
        <span>· <strong>Allowed:</strong> ✓ {delegation.allowedCapabilities.join(', ')}</span>
        <span>· <strong>Denied:</strong> ✗ {delegation.deniedCapabilities.join(', ')}</span>
      </div>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, fontSize: 12 }}>
        {delegation.steps.map((step) => <span key={step.step} style={{ padding: '4px 8px', borderRadius: 999, background: 'rgba(16, 185, 129, 0.14)' }}>{step.step} {step.tool} {step.status === 'skipped' ? '— skipped (not connected)' : '✓'}</span>)}
      </div>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: 8 }}>
        {delegation.priorities.map((item) => <div key={`${item.tier}-${item.title}`} style={{ padding: '8px 10px', borderRadius: 10, background: 'rgba(255,255,255,0.04)' }}>
          <small style={{ fontWeight: 700 }}>{item.tier} · {item.source}</small>
          <div style={{ fontWeight: 600, marginTop: 2 }}>{item.title}</div>
          <small style={{ opacity: 0.78 }}>{item.detail}</small>
        </div>)}
      </div>
    </div>}
  </div>;
}

function AssistantView({ actions, sendPrompt, turns, busy, prompt, setPrompt, onRemind }: { actions: InboxAction[]; sendPrompt: (prompt: string, surface?: 'web' | 'alexa') => void; turns: Turn[]; busy: boolean; prompt: string; setPrompt: (prompt: string) => void; onRemind: (item: InboxAction) => void }) {
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    listRef.current?.scrollTo({ top: listRef.current.scrollHeight, behavior: 'smooth' });
  }, [turns, busy]);
  return <section className="view assistant-view">
    <div className="assistant-heading"><div><p className="kicker"><Sparkles /> YOUR DAY, RESOLVED</p><h1>What can I help with?</h1><p>Ask naturally. SHARMAN plans the work, checks permissions, and shows every tool it uses.</p></div><div className="privacy-note"><ShieldCheck /><span><strong>Private by design</strong><small>Mailbox content stays local</small></span></div></div>
    <div className="chat-stage">
      <div className="chat-scroll" ref={listRef}>
        {turns.length === 0 ? <div className="empty-conversation"><div className="ambient-orb"><Sparkles /></div><h2>What needs your attention?</h2><p>I can review tasks and actionable email, delegate a temporary DailyPlannerAgent, and prepare approved reminders.</p><div className="prompt-grid">{quickPrompts.map((item) => <button onClick={() => sendPrompt(item)} key={item}><span>{item}</span><ChevronRight /></button>)}</div></div> : <div className="turn-list">
          {turns.map((turn) => <motion.div initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} className={`turn ${turn.role}`} key={turn.id}>
            {turn.role === 'assistant' && <div className="assistant-avatar"><Sparkles /></div>}
            <div><small>{turn.role === 'user' ? 'YOU' : 'SHARMAN'}</small>{turn.trace && <ExecutionTrace trace={turn.trace} />}<p>{turn.text}</p>
              {turn.trace?.result.messages && <MailMessages messages={turn.trace.result.messages} />}
              {turn.trace?.result.items?.map((item) => <InboxCard item={item} compact onRemind={onRemind} key={item.id} />)}
            </div>
          </motion.div>)}
          {busy && <div className="turn assistant" role="status"><div className="assistant-avatar"><Sparkles /></div><div className="supervisor-pending"><div className="thinking"><i /><i /><i /></div><strong>Supervisor is evaluating your request</strong><small>Checking tools and delegated permissions. Awaiting a confirmed execution result.</small></div></div>}
        </div>}
      </div>
      <form className="composer" onSubmit={(event) => { event.preventDefault(); if (prompt.trim()) sendPrompt(prompt.trim()); }}><button type="button" className="composer-plus" aria-label="Add"><Plus /></button><input value={prompt} onChange={(event) => setPrompt(event.target.value)} placeholder="Ask SHARMAN anything…" autoFocus /><button type="button" className="mic-button" aria-label="Speak"><Mic /></button><button className="send-button" disabled={busy || !prompt.trim()} aria-label="Send"><Send /></button><div className="composer-foot"><span><LockKeyhole /> Permission-aware tools</span><span>{actions.length} actionable emails</span></div></form>
    </div>
  </section>;
}

function MailMessages({ messages }: { messages: NonNullable<ChatResult['result']['messages']> }) {
  return <div className="mail-message-list">{messages.map((message) => <article className="mail-message" key={message.id}><small>GMAIL · READ ONLY · {formatDate(message.receivedAt)}</small><h3>{message.subject || '(No subject)'}</h3><span>{message.sender}</span><p>{message.snippet || 'No preview available.'}</p>{/^[a-f0-9]{1,128}$/i.test(message.threadId) && <a href={`https://mail.google.com/mail/u/0/#inbox/${message.threadId}`} target="_blank" rel="noreferrer">Open in Gmail <ExternalLink /></a>}</article>)}</div>;
}

function InboxView({ actions, onRemind, status, sample, onSample }: { actions: InboxAction[]; onRemind: (item: InboxAction) => void; status: BridgeStatus | null; sample: boolean; onSample: (sample: boolean) => void }) {
  const connected = status?.mailbox?.connected && !status.mailbox.needsReconnect;
  return <section className="view"><PageHeading eyebrow="ACTION INBOX" title="Your action inbox" copy="Review email actions and upcoming deadlines." />
    <div className="mailbox-notice" role="status"><Mail /><div><strong>{sample ? 'Sample inbox preview — not your emails' : connected ? 'Private Gmail connection · read only' : status?.mailbox?.needsReconnect ? 'Gmail needs reconnecting' : 'Gmail is not connected to this console'}</strong><p>{sample ? 'These examples demonstrate action extraction. They are not recent or unread Gmail and cannot create personal reminders.' : connected ? 'Action items come from the local Gmail index. Ask for recent or unread emails to read actual message metadata.' : 'Connect Gmail in the Windows app to use your real inbox. The public demo does not have access to your Google account.'}</p></div><button className="secondary-button" onClick={() => onSample(!sample)}>{sample ? 'Leave sample preview' : 'Preview sample inbox'}</button></div>
    <div className="metric-row"><Metric label="Needs action" value={String(actions.filter((item) => item.status === 'suggested').length)} tone="violet" /><Metric label="Due in 48 hours" value={String(actions.filter((item) => item.deadline && new Date(item.deadline).getTime() - Date.now() < 172_800_000).length)} tone="amber" /><Metric label="Mailbox permission" value="Read only" tone="green" /></div>
    <div className="section-bar"><h2>{sample ? 'Example action items' : 'Your action items'}</h2><span>{sample ? 'Sample data' : connected ? 'Private Gmail index' : 'No mailbox connected'}</span></div><div className="inbox-list">{actions.map((item) => <InboxCard item={item} onRemind={onRemind} key={item.id} />)}{actions.length === 0 && <p className="mailbox-empty">{connected ? 'No indexed action items. This does not mean your inbox is empty.' : 'No real emails are displayed. Connect Gmail privately or explicitly preview samples.'}</p>}</div>
  </section>;
}

function TasksView({ tasks, status, onCreate, onAssistant }: {
  tasks: Task[];
  status: BridgeStatus | null;
  onCreate: (title: string, dueAt: string | null) => Promise<void>;
  onAssistant: () => void;
}) {
  const [filter, setFilter] = useState<Task['status']>('open');
  const [creating, setCreating] = useState(false);
  const [title, setTitle] = useState('');
  const [deadline, setDeadline] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pending = useRef(false);
  const newButton = useRef<HTMLButtonElement>(null);
  const open = tasks.filter((task) => task.status === 'open').sort((a, b) =>
    (a.dueAt ? new Date(a.dueAt).getTime() : Infinity) - (b.dueAt ? new Date(b.dueAt).getTime() : Infinity));
  const completed = tasks.filter((task) => task.status === 'completed');
  const visible = filter === 'open' ? open : completed;
  const next = open[0];
  const sample = status?.personalStorage === 'web_json';
  const closeForm = () => { setCreating(false); setError(null); newButton.current?.focus(); };
  return <section className="view tasks-view">
    <PageHeading eyebrow="YOUR TASKS" title="Make room for what matters." copy="A clear place for the things you choose to do." action={<button ref={newButton} className="primary-button" disabled={!status || creating} onClick={() => setCreating(true)}><Plus /> New task</button>} />
    {sample && <div className="task-workspace-note"><ShieldCheck /><span><strong>Public demo workspace</strong> · Tasks are shared and may reset. Try sample information, not private details.</span></div>}
    {creating && <form className="task-create" aria-label="Create task" onSubmit={async (event) => {
      event.preventDefault();
      if (pending.current || title.trim().length < 2) return;
      pending.current = true; setSaving(true); setError(null);
      try {
        await onCreate(title.trim(), deadline ? new Date(deadline).toISOString() : null);
        setTitle(''); setDeadline(''); setFilter('open'); closeForm();
      } catch (failure) {
        setError(`${failure instanceof Error ? failure.message : 'Could not confirm this task.'} Your text is kept. Check your tasks before retrying.`);
      } finally { pending.current = false; setSaving(false); }
    }}>
      <div className="task-form-heading"><h2>What would you like to get done?</h2><button type="button" className="icon-button" disabled={saving} onClick={closeForm} aria-label="Close task form"><X /></button></div>
      <div className="task-form-fields"><label>Task name<input autoFocus required minLength={2} maxLength={240} value={title} onChange={(event) => setTitle(event.target.value)} disabled={saving} placeholder="Add something that matters to you" /></label><label>Due date <span>(optional · your timezone)</span><input type="datetime-local" value={deadline} onChange={(event) => setDeadline(event.target.value)} disabled={saving} /></label></div>
      {error && <p className="task-form-error" role="alert">{error}</p>}
      <div className="task-form-actions"><button type="button" className="secondary-button" disabled={saving} onClick={closeForm}>Cancel</button><button className="primary-button" disabled={saving || title.trim().length < 2}>{saving ? 'Saving…' : 'Save task'}</button></div>
    </form>}
    <div className="task-board"><div className="task-list-panel">
      <div className="task-list-heading"><div className="task-filters" role="group" aria-label="Task status"><button aria-pressed={filter === 'open'} onClick={() => setFilter('open')}>Upcoming <span>{open.length}</span></button><button aria-pressed={filter === 'completed'} onClick={() => setFilter('completed')}>Completed <span>{completed.length}</span></button></div><span className="task-list-caption">{filter === 'open' ? 'Deadline order' : 'Task history'}</span></div>
      {!status ? <div className="tasks-empty" role="status"><Unplug /><h2>Tasks are unavailable</h2><p>The bridge is not connected. Your saved tasks have not been cleared. Reconnect and reload to see them.</p></div> : visible.length === 0 ? <div className="tasks-empty">
        <div className="tasks-empty-icon">{filter === 'open' ? <ListTodo /> : <Check />}</div>
        <span className="focus-kicker">{filter === 'open' ? 'A LITTLE SPACE TO BEGIN' : 'YOUR PROGRESS LIVES HERE'}</span>
        <h2>{filter === 'open' ? 'Your next step starts here.' : 'No completed tasks yet.'}</h2>
        <p>{filter === 'open' ? 'Add your first task, or ask SHARMAN to help you decide what needs attention. No pre-filled tasks, just your next steps.' : 'Completed tasks will appear here when they are marked done through a connected tool.'}</p>
        {filter === 'open' && <button className="secondary-button" onClick={() => { setCreating(true); window.scrollTo({ top: 0, behavior: 'smooth' }); }} disabled={creating}><Plus /> Add your first task</button>}
      </div> : <div className="task-list">{visible.map((task, index) => <motion.article className="task-row" initial={{ opacity: 0, x: -8 }} animate={{ opacity: 1, x: 0 }} transition={{ delay: index * .04 }} key={task.id}>
        <span role="img" className={`task-status-icon ${task.status}`} aria-label={task.status === 'completed' ? 'Completed task' : 'Open task'}>{task.status === 'completed' ? <Check /> : <ListTodo />}</span>
        <div><strong>{task.title}</strong><small><Clock3 /> {formatDate(task.dueAt)}</small></div>
        <span className={`source-tag ${task.source}`}>{task.source === 'user' ? 'You' : task.source === 'alexa' ? 'Alexa+' : 'Gmail'}</span>
      </motion.article>)}</div>}
    </div><aside className="focus-card"><span className="focus-kicker">{next ? 'UP NEXT' : 'AT YOUR PACE'}</span><div className="focus-ring">{next ? <><span>{open.length}</span><small>open</small></> : <Sparkles />}</div><h3>{next?.title || 'Less noise. More intention.'}</h3><p>{next ? formatDate(next.dueAt) : 'Your list starts with you, not with a demo.'}</p>{next && <small className="focus-reason">{next.dueAt ? 'Your earliest scheduled task' : 'No deadline set'}</small>}<button onClick={onAssistant}>Ask SHARMAN <ChevronRight /></button><div className="task-safety"><ShieldCheck /> You choose. SHARMAN assists.</div></aside></div>
  </section>;
}

function ActivityView({ events }: { events: AuditEvent[] }) {
  return <section className="view"><PageHeading eyebrow="ACTIVITY & AUDIT" title="Every decision has a receipt." copy="See what interpreted your request, which parent/child agent ran, the permission boundary, and the outcome." />
    <div className="audit-table" role="region" aria-label="Activity history" tabIndex={0}><div className="audit-header"><span>Time</span><span>Actor</span><span>Action</span><span>Permission</span><span>Result</span></div>{events.map((event) => <div className="audit-row" key={event.id}><time>{relativeTime(event.at)}</time><span className="actor"><Bot />{event.childAgent ? `${event.actor} → ${event.childAgent}` : event.actor}</span><strong>{event.action}{event.tool ? ` · ${event.tool}` : ''}</strong><span className={`permission ${event.permission.toLowerCase()}`}><LockKeyhole />{event.permission.replace('_', ' ')}</span><p>{event.result}{event.denialReason ? ` (Required: ${event.requiredPermission}, Ceiling: ${event.delegatedCeiling} — ${event.denialReason})` : ''}</p></div>)}</div>
  </section>;
}

function ConnectionsView({ status, onSelectMode }: { status: BridgeStatus | null; onSelectMode: (mode: ProcessingMode) => void }) {
  const activeMode: ProcessingMode = status?.processingMode ?? 'local_only';
  const modes: Array<{ id: ProcessingMode; label: string; copy: string }> = [
    { id: 'local_only', label: 'Local Only (Default)', copy: 'Handle requests locally. Cloud inference stays off.' },
    { id: 'hybrid', label: 'Hybrid', copy: 'Local extraction first; cloud reasoning only when explicitly requested.' },
    { id: 'cloud_allowed', label: 'Cloud Allowed', copy: 'Allow supported cloud services when requested. Permissions still apply.' },
  ];
  const connections = [
    { name: 'SHARMAN MCP', detail: status?.mcpEndpoint || 'Start the local bridge', state: Boolean(status), Icon: Server },
    { name: 'Gmail', detail: status?.mailbox?.connected ? 'Private native Gmail · read-only metadata' : 'Not connected here · connect in Windows app', state: Boolean(status?.mailbox?.connected && !status.mailbox.needsReconnect), Icon: Mail },
    { name: 'Alexa+', detail: 'Try requests through the Alexa+ preview', state: false, label: 'Simulated', Icon: Sparkles },
    { name: 'Amazon Bedrock', detail: status?.aws?.bedrock.connectionState === 'connected' ? `${status.aws.bedrock.modelId === 'global.amazon.nova-2-lite-v1:0' ? 'Nova 2 Lite' : status.aws.bedrock.modelId} · ${status.aws.bedrock.region}` : status?.aws?.bedrock.configured && status.aws.bedrock.connectionState !== 'unavailable' ? 'Cloud explanations available on request' : 'Local explanations are active', state: status?.aws?.bedrock.connectionState === 'connected', label: status?.aws?.bedrock.connectionState === 'connected' ? 'Connected' : status?.aws?.bedrock.connectionState === 'unavailable' ? 'Unavailable' : status?.aws?.bedrock.configured ? 'Not connected' : 'Off', Icon: Cloud },
    { name: 'Windows notifications', detail: 'Reminders are delivered by the Windows app', state: status?.personalStorage === 'native_sqlite', label: status?.personalStorage === 'native_sqlite' ? 'Available' : 'Windows app only', Icon: BellRing },
  ];
  return <section className="view"><PageHeading eyebrow="CONNECTIONS & PROCESSING MODE" title="Useful data. Explicit boundaries." copy="Every source declares what it can read, suggest, and change before an agent can use it." />
    <div className="section-bar"><h2>Processing mode</h2><span>{activeMode.replace('_', ' ')}</span></div>
    <div className="connection-grid" role="group" aria-label="Processing mode">{modes.map(({ id, label, copy }) => <article key={id} style={{ cursor: 'pointer', outline: activeMode === id ? '2px solid rgba(16, 185, 129, 0.7)' : 'none' }} onClick={() => onSelectMode(id)}><div className="connection-icon"><ShieldCheck /></div><div><h3>{label}</h3><p>{copy}</p></div><span className={activeMode === id ? 'connected' : 'setup'}>{activeMode === id ? <><Check /> Active</> : 'Select'}</span></article>)}</div>
    <div className="section-bar" style={{ marginTop: 18 }}><h2>Connections</h2><span>Integration status</span></div>
    <div className="connection-grid">{connections.map(({ name, detail, state, label, Icon }) => <article key={name}><div className="connection-icon"><Icon /></div><div><h3>{name}</h3><p>{detail}</p></div><span className={state ? 'connected' : 'setup'}>{label || (state ? <><Check /> Ready</> : <><Unplug /> Setup needed</>)}</span></article>)}</div>
    <div className="boundary-card"><ShieldCheck /><div><span>THE SHARMAN BOUNDARY</span><h2>The model proposes. SHARMAN authorizes. Tools execute.</h2><p>Background Gmail work is capped at READ + SUGGEST. Child agents can never exceed their parent’s delegated permissions.</p></div></div>
  </section>;
}

const alexaCommands = [
  'What tasks do I have today?',
  'Remind me at 8 PM to solve LeetCode.',
  'What needs my attention today?',
  "Open today's LeetCode.",
  'Remind me in 2 minutes to take a break.',
];

function AlexaDemo({
  actions,
  onRemind,
  runAlexa,
  result,
  busy,
}: {
  actions: InboxAction[];
  onRemind: (item: InboxAction) => void;
  runAlexa: (cmd: string) => void;
  result: ChatResult | null;
  busy: boolean;
}) {
  const [selectedCommand, setSelectedCommand] = useState(alexaCommands[0]);
  const [mcpConnection, setMcpConnection] = useState<McpConnection | null>(null);
  const [mcpError, setMcpError] = useState('');
  const connect = () => { setMcpError(''); void connectAlexaMcp().then(setMcpConnection).catch((error: unknown) => setMcpError(error instanceof Error ? error.message : 'MCP connection failed.')); };
  useEffect(() => { connect(); }, []);

  const handleRun = (cmd: string) => {
    setSelectedCommand(cmd);
    runAlexa(cmd);
  };

  return (
    <section className="alexa-page">
      <div className="alexa-aurora" />
      <header className="alexa-header">
        <Brand />
        <div>
          <span>SIMULATED ALEXA+ EXPERIENCE</span>
          <b>Ask, review and act with permission</b>
        </div>
      </header>

      <div className="alexa-stage">
        <div
          role="status"
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: '8px',
            padding: '6px 14px',
            borderRadius: '99px',
            background: 'rgba(255, 255, 255, 0.035)',
            border: '1px solid var(--line)',
            color: '#cbd5e1',
            fontSize: '12.5px',
            marginBottom: '20px',
          }}
        >
          <span style={{ width: 8, height: 8, borderRadius: '50%', background: mcpConnection ? 'var(--green)' : 'var(--amber)', boxShadow: mcpConnection ? '0 0 10px var(--green)' : '0 0 10px var(--amber)' }} />
          {mcpError ? (
            <>
              <span style={{ color: 'var(--red-hi)' }}>{mcpError}</span>
              <button onClick={connect} className="primary-small" style={{ padding: '2px 8px', fontSize: '11px', marginLeft: 6 }}>Reconnect MCP</button>
            </>
          ) : mcpConnection ? (
            <>Connected to SHARMAN · {mcpConnection.tools.length} tools available</>
          ) : (
            'Connecting to SHARMAN…'
          )}
        </div>

        <div style={{ display: 'flex', flexWrap: 'wrap', gap: '10px', justifyContent: 'center', marginBottom: '24px', maxWidth: '800px' }}>
          {alexaCommands.map((cmd) => (
            <button
              key={cmd}
              onClick={() => handleRun(cmd)}
              disabled={busy}
              style={{
                padding: '9px 18px',
                borderRadius: '999px',
                border: selectedCommand === cmd ? '1px solid rgba(139, 92, 246, 0.6)' : '1px solid rgba(255, 255, 255, 0.1)',
                background: selectedCommand === cmd ? 'rgba(139, 92, 246, 0.22)' : 'rgba(255, 255, 255, 0.035)',
                color: selectedCommand === cmd ? '#ffffff' : '#cbd5e1',
                fontSize: '13.5px',
                fontWeight: selectedCommand === cmd ? 650 : 500,
                cursor: 'pointer',
                boxShadow: selectedCommand === cmd ? '0 4px 16px rgba(139, 92, 246, 0.25)' : 'none',
                transition: 'all 0.18s cubic-bezier(0.16, 1, 0.3, 1)',
              }}
            >
              {cmd}
            </button>
          ))}
        </div>

        <div className={`voice-orb ${busy ? 'listening' : result ? 'answered' : ''}`}>
          <div><Sparkles /></div>
          <i /><i /><i />
        </div>
        <p className="voice-state">{busy ? 'MCP Bridge executing…' : result ? 'SHARMAN responded via MCP' : 'Ready when you are'}</p>
        <h1 style={{ maxWidth: '720px' }}>“{selectedCommand}”</h1>
        <button className="voice-trigger" disabled={busy} onClick={() => handleRun(selectedCommand)}>
          <Mic /> {busy ? 'Calling MCP bridge…' : result ? 'Run again' : 'Run voice command'}
        </button>

        <AnimatePresence>
          {result && (
            <motion.div
              className="alexa-response"
              initial={{ opacity: 0, y: 24, scale: 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              transition={{ duration: 0.2 }}
            >
              <div className="alexa-answer">
                <span><Sparkles /> Alexa+ via SHARMAN Supervisor</span>
                <p>{result.answer}</p>
              </div>

              <div
                style={{
                  display: 'grid',
                  gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
                  gap: '14px',
                  padding: '16px 24px',
                  background: 'rgba(255,255,255,0.025)',
                  borderTop: '1px solid var(--line)',
                  borderBottom: '1px solid var(--line)',
                  fontSize: '13px',
                }}
              >
                <div>
                  <small style={{ color: 'var(--violet-hi)', textTransform: 'uppercase', letterSpacing: '0.09em', fontWeight: 700, display: 'block', marginBottom: '4px', fontSize: '11px' }}>
                    1. MCP Request
                  </small>
                  <code style={{ fontSize: '12.5px', color: '#e2e8f0', background: 'rgba(255,255,255,0.05)', padding: '2px 6px', borderRadius: '4px' }}>
                    {result.transport === 'streamable_http_mcp' ? 'POST /mcp → tools/call' : 'SHARMAN local route'}
                  </code>
                </div>
                <div>
                  <small style={{ color: 'var(--violet-hi)', textTransform: 'uppercase', letterSpacing: '0.09em', fontWeight: 700, display: 'block', marginBottom: '4px', fontSize: '11px' }}>
                    2. Tool Selected
                  </small>
                  <strong style={{ color: '#ffffff', fontSize: '13.5px' }}>{result.tool || 'sharman-supervisor'}</strong>
                </div>
                <div>
                  <small style={{ color: 'var(--violet-hi)', textTransform: 'uppercase', letterSpacing: '0.09em', fontWeight: 700, display: 'block', marginBottom: '4px', fontSize: '11px' }}>
                    3. Permission Level
                  </small>
                  <span
                    style={{
                      display: 'inline-flex',
                      alignItems: 'center',
                      gap: '5px',
                      fontWeight: 650,
                      fontSize: '12.5px',
                      padding: '3px 9px',
                      borderRadius: '99px',
                      background:
                        result.permission === 'WRITE_APPROVED'
                          ? 'rgba(245, 158, 11, 0.12)'
                          : result.permission === 'LOW_RISK'
                          ? 'rgba(16, 185, 129, 0.12)'
                          : 'rgba(56, 189, 248, 0.12)',
                      color:
                        result.permission === 'WRITE_APPROVED'
                          ? 'var(--amber-hi)'
                          : result.permission === 'LOW_RISK'
                          ? 'var(--green-hi)'
                          : 'var(--blue-hi)',
                      border: `1px solid ${
                        result.permission === 'WRITE_APPROVED'
                          ? 'rgba(245, 158, 11, 0.3)'
                          : result.permission === 'LOW_RISK'
                          ? 'rgba(16, 185, 129, 0.3)'
                          : 'rgba(56, 189, 248, 0.3)'
                      }`,
                    }}
                  >
                    <LockKeyhole style={{ width: 12, height: 12 }} /> {result.permission}
                  </span>
                </div>
                <div>
                  <small style={{ color: 'var(--violet-hi)', textTransform: 'uppercase', letterSpacing: '0.09em', fontWeight: 700, display: 'block', marginBottom: '4px', fontSize: '11px' }}>
                    4. Execution Result
                  </small>
                  <strong style={{ color: 'var(--green-hi)', fontSize: '13.5px' }}>
                    {result.tool === 'sharman_get_tasks'
                      ? `${result.result.tasks?.length ?? 0} active tasks`
                      : result.tool === 'sharman_create_reminder'
                      ? 'Reminder scheduled'
                      : result.tool === 'DailyPlannerAgent'
                      ? `${result.result.delegation?.stepsUsed}/${result.result.delegation?.maxSteps} steps · Terminated`
                      : result.tool === 'sharman_open_allowed_url'
                      ? 'Link ready'
                      : 'Success'}
                  </strong>
                </div>
              </div>

              <div style={{ padding: '22px 26px', background: 'rgba(16,21,34,0.96)' }}>
                {result.tool === 'sharman_get_tasks' && result.result.tasks && (
                  <div style={{ display: 'grid', gap: '10px' }}>
                    <div style={{ fontSize: '13px', fontWeight: 700, color: 'var(--muted)', letterSpacing: '0.04em', textTransform: 'uppercase', marginBottom: '4px' }}>
                      Your tasks ({result.result.tasks.length} items)
                    </div>
                    {result.result.tasks.slice(0, 4).map((task) => (
                      <div
                        key={task.id}
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'space-between',
                          padding: '12px 16px',
                          borderRadius: '12px',
                          background: 'rgba(255,255,255,0.035)',
                          border: '1px solid rgba(255,255,255,0.08)',
                          fontSize: '14px',
                        }}
                      >
                        <div style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
                          <span style={{ color: 'var(--green)', display: 'grid', placeItems: 'center' }}><Check style={{ width: 16, height: 16 }} /></span>
                          <strong style={{ color: '#ffffff' }}>{task.title}</strong>
                        </div>
                        <small style={{ color: 'var(--muted)', fontSize: '12.5px' }}>{formatDate(task.dueAt)}</small>
                      </div>
                    ))}
                  </div>
                )}

                {result.tool === 'sharman_create_reminder' && result.result.reminder && (
                  <div
                    style={{
                      padding: '18px 22px',
                      borderRadius: '16px',
                      background: 'rgba(245, 158, 11, 0.08)',
                      border: '1px solid rgba(245, 158, 11, 0.28)',
                      display: 'grid',
                      gap: '8px',
                    }}
                  >
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                      <span style={{ color: 'var(--amber-hi)', fontSize: '12px', fontWeight: 750, letterSpacing: '0.08em', textTransform: 'uppercase' }}>
                        Reminder scheduled
                      </span>
                      <span style={{ fontSize: '12px', color: 'var(--green-hi)', display: 'flex', alignItems: 'center', gap: '5px', fontWeight: 600 }}>
                        <Check style={{ width: 14, height: 14 }} /> Saved
                      </span>
                    </div>
                    <div style={{ fontSize: '16px', fontWeight: 700, color: '#ffffff' }}>
                      {result.result.reminder.title}
                    </div>
                    <div style={{ fontSize: '13.5px', color: '#cbd5e1', display: 'flex', alignItems: 'center', gap: '8px' }}>
                      <Clock3 style={{ width: 15, height: 15, color: 'var(--amber)' }} />
                      Scheduled for: <strong style={{ color: '#ffffff' }}>{formatDate(result.result.reminder.remindAt)}</strong>
                    </div>
                    <small style={{ color: 'var(--muted)', fontSize: '12px', fontFamily: "'DM Mono', monospace", marginTop: '4px' }}>
                      {result.result.reminder.storage === 'native_sqlite' ? 'Delivery: Windows app' : 'Saved in this workspace'}
                    </small>
                  </div>
                )}

                {result.tool === 'DailyPlannerAgent' && result.result.delegation && (
                  <div style={{ display: 'grid', gap: '14px' }}>
                    <div style={{ display: 'flex', flexWrap: 'wrap', gap: '10px', fontSize: '13px' }}>
                      <span><strong>Ceiling:</strong> <span style={{ color: 'var(--blue-hi)' }}>{result.result.delegation.permissionCeiling}</span></span>
                      <span>· <strong>Budget:</strong> {result.result.delegation.stepsUsed}/{result.result.delegation.maxSteps} steps</span>
                      <span>· <strong>Allowed:</strong> ✓ {result.result.delegation.allowedCapabilities.join(', ')}</span>
                      <span>· <strong>Denied:</strong> ✗ {result.result.delegation.deniedCapabilities.join(', ')}</span>
                    </div>
                    <div style={{ display: 'flex', flexWrap: 'wrap', gap: '8px', fontSize: '12.5px' }}>
                      {result.result.delegation.steps.map((step) => (
                        <span key={step.step} style={{ padding: '4px 10px', borderRadius: 999, background: 'rgba(16, 185, 129, 0.16)', color: 'var(--green-hi)', fontWeight: 600 }}>
                          {step.step} {step.tool} ✓
                        </span>
                      ))}
                    </div>
                    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: '10px', marginTop: '6px' }}>
                      {result.result.delegation.priorities.map((item) => (
                        <div key={`${item.tier}-${item.title}`} style={{ padding: '12px 14px', borderRadius: 12, background: 'rgba(255,255,255,0.04)', border: '1px solid rgba(255,255,255,0.06)' }}>
                          <small style={{ fontWeight: 750, color: item.tier === 'CRITICAL' ? '#f43f5e' : 'var(--amber)', fontSize: '11.5px', textTransform: 'uppercase', letterSpacing: '0.06em' }}>
                            {item.tier} · {item.source}
                          </small>
                          <div style={{ fontWeight: 650, marginTop: 4, color: '#ffffff', fontSize: '13.5px' }}>{item.title}</div>
                          <small style={{ color: 'var(--muted)', fontSize: '12px', display: 'block', marginTop: 2 }}>{item.detail}</small>
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                {result.tool === 'sharman_open_allowed_url' && (
                  <div
                    style={{
                      padding: '18px 22px',
                      borderRadius: '16px',
                      background: 'rgba(16, 185, 129, 0.08)',
                      border: '1px solid rgba(16, 185, 129, 0.28)',
                      display: 'grid',
                      gap: '10px',
                    }}
                  >
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                      <span style={{ color: 'var(--green-hi)', fontSize: '12px', fontWeight: 750, letterSpacing: '0.08em', textTransform: 'uppercase' }}>
                        Link ready
                      </span>
                      <span style={{ fontSize: '12px', color: 'var(--blue-hi)', fontWeight: 600 }}>Domain: leetcode.com</span>
                    </div>
                    <div style={{ fontSize: '14.5px', color: '#ffffff' }}>
                      Target: <code style={{ color: 'var(--green-hi)', background: 'rgba(0,0,0,0.3)', padding: '2px 8px', borderRadius: '4px' }}>{result.result.url || 'https://leetcode.com/problemset/'}</code>
                    </div>
                    <div style={{ fontSize: '13px', color: 'var(--muted)', lineHeight: '1.6' }}>
                      Open the link when you are ready.
                    </div>
                    <div style={{ marginTop: '6px' }}>
                      <a
                        href={result.result.url || 'https://leetcode.com/problemset/'}
                        target="_blank"
                        rel="noreferrer"
                        className="primary-small"
                        style={{ display: 'inline-flex', textDecoration: 'none', width: 'fit-content', padding: '10px 16px', fontSize: '13px' }}
                      >
                        Launch LeetCode (User Confirmed) <ExternalLink style={{ width: 14, height: 14, marginLeft: 6 }} />
                      </a>
                    </div>
                  </div>
                )}

                {result.tool === 'sharman_get_action_items' && (
                  <div className="alexa-results">
                    {(result.result.items || actions).slice(0, 2).map((item) => (
                      <div key={item.id}>
                        <span className="mail-dot"><Mail /></span>
                        <section>
                          <small>{item.sender}</small>
                          <strong>{item.subject}</strong>
                          <p>{formatDate(item.deadline)}</p>
                        </section>
                        {item.sample ? <span className="sample-email-label">Sample email · not your inbox</span> : <button onClick={() => onRemind(item)}><BellRing /> Remind me</button>}
                      </div>
                    ))}
                  </div>
                )}
                {result.result.messages && <MailMessages messages={result.result.messages} />}
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      <footer className="alexa-footer">
        <span><Radio /> Simulated Alexa+ surface</span>
        <span>Tools require permission</span>
        <span>Demo mailbox data</span>
      </footer>
    </section>
  );
}

function PageHeading({ eyebrow, title, copy, action }: { eyebrow: string; title: string; copy: string; action?: React.ReactNode }) { return <div className="page-heading"><div><p>{eyebrow}</p><h1>{title}</h1><span>{copy}</span></div>{action}</div>; }
function Metric({ label, value, tone }: { label: string; value: string; tone: string }) { return <article className={`metric ${tone}`}><span>{label}</span><strong>{value}</strong><i /></article>; }

export default function App() {
  const initialView: View = window.location.pathname.startsWith('/alexa') ? 'alexa' : window.location.pathname.startsWith('/home') ? 'home' : window.location.pathname.startsWith('/tasks') ? 'tasks' : 'assistant';
  const [view, setViewState] = useState<View>(initialView);
  const [status, setStatus] = useState<BridgeStatus | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [actions, setActions] = useState<InboxAction[]>([]);
  const [inboxSample, setInboxSample] = useState(false);
  const [audit, setAudit] = useState<AuditEvent[]>([]);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [prompt, setPrompt] = useState('');
  const [busy, setBusy] = useState(false);
  const [alexaResult, setAlexaResult] = useState<ChatResult | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);

  const refresh = async () => {
    try {
      const [nextStatus, nextTasks, nextActions, nextAudit] = await Promise.all([getStatus(), getTasks(), getActions(inboxSample), getAudit()]);
      setStatus(nextStatus); setTasks(nextTasks.tasks); setActions(nextActions.items); setAudit(nextAudit.events);
    } catch { setStatus(null); }
  };
  useEffect(() => { void refresh(); }, []);
  useEffect(() => { if (!toast) return; const timer = window.setTimeout(() => setToast(null), 3600); return () => window.clearTimeout(timer); }, [toast]);

  const setView = (next: View) => {
    setViewState(next);
    window.history.replaceState(null, '', next === 'alexa' ? '/alexa' : next === 'home' ? '/home' : next === 'tasks' ? '/tasks' : '/');
  };

  const sendPrompt = async (message: string, surface: 'web' | 'alexa' = 'web') => {
    setBusy(true); setPrompt('');
    if (surface === 'web') setTurns((current) => [...current, { id: crypto.randomUUID(), role: 'user', text: message }]);
    try {
      const result = await chat(message, surface);
      if (surface === 'alexa') setAlexaResult(result);
      else setTurns((current) => [...current, { id: crypto.randomUUID(), role: 'assistant', text: result.answer, trace: result }]);
      await refresh();
    } catch (error) { setToast(error instanceof Error ? error.message : 'SHARMAN could not confirm the command result.'); }
    finally { setBusy(false); }
  };

  const remind = async (item: InboxAction) => {
    if (!item.deadline || item.sample) return;
    const time = new Date(new Date(item.deadline).getTime() - 2 * 60 * 60 * 1000).toISOString();
    try { await createReminder(item.subject, time, item.id); setToast(`Reminder created for ${formatDate(time)}.`); await refresh(); }
    catch { setToast('The MCP bridge is offline. Start it before creating a reminder.'); }
  };

  const updateMode = async (mode: ProcessingMode) => {
    try {
      await setProcessingMode(mode);
      setToast(`Processing mode set to ${mode.replace('_', ' ')}.`);
      await refresh();
    } catch {
      setToast('Start the SHARMAN MCP bridge on port 4179 to change processing mode.');
    }
  };

  const addTask = async (title: string, dueAt: string | null) => {
    const result = await createTask(title, dueAt);
    setTasks((current) => [result.task, ...current.filter((task) => task.id !== result.task.id)]);
    setToast(result.created ? 'Task added to your list.' : 'This task is already on your list.');
    await refresh();
  };

  const selectInboxSample = async (sample: boolean) => {
    try { const result = await getActions(sample); setActions(result.items); setInboxSample(sample); }
    catch { setToast('Could not load the inbox source. Nothing was substituted.'); }
  };

  const content = useMemo(() => {
    if (view === 'home') return <HomeView onChanged={() => void refresh()} notify={setToast} />;
    if (view === 'inbox') return <InboxView actions={actions} onRemind={remind} status={status} sample={inboxSample} onSample={(sample) => void selectInboxSample(sample)} />;
    if (view === 'tasks') return <TasksView tasks={tasks} status={status} onCreate={addTask} onAssistant={() => setView('assistant')} />;
    if (view === 'activity') return <ActivityView events={audit} />;
    if (view === 'connections') return <ConnectionsView status={status} onSelectMode={(mode) => void updateMode(mode)} />;
    if (view === 'alexa') return <AlexaDemo actions={actions} onRemind={remind} runAlexa={(cmd) => void sendPrompt(cmd, 'alexa')} result={alexaResult} busy={busy} />;
    return <AssistantView actions={actions} sendPrompt={(value) => void sendPrompt(value)} turns={turns} busy={busy} prompt={prompt} setPrompt={setPrompt} onRemind={remind} />;
  }, [actions, alexaResult, audit, busy, prompt, status, tasks, turns, view, inboxSample]);

  return <Shell view={view} setView={setView} status={status} menuOpen={menuOpen} setMenuOpen={setMenuOpen}>{content}<AnimatePresence>{toast && <motion.div className="toast" initial={{ opacity: 0, y: 18 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: 12 }}><Check />{toast}</motion.div>}</AnimatePresence></Shell>;
}
