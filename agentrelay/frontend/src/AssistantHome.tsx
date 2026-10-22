import { useEffect, useMemo, useRef, useState } from 'react'
import type { FormEvent } from 'react'
import { ChatIcon, MessageText } from './AssistantUI'
import {
  askSharman,
  connectGmail,
  createAssistantConversation,
  createEmailReminder,
  disconnectGmail,
  dismissEmailAction,
  dismissPersonalReminder,
  executeAssistantAction,
  forgetAssistantMemory,
  getAssistantAudit,
  getAssistantHome,
  getAssistantMemory,
  getInboxDashboard,
  getPersonalDashboard,
  installLocalAiRuntime,
  openGmailMessage,
  pullRecommendedLocalModel,
  refreshGmailInbox,
  setPersonalTaskCompleted,
  type AssistantAction,
  type AssistantAuditRecord,
  type AssistantHomeSnapshot,
  type AssistantMemorySnapshot,
  type AssistantResponse,
  type EmailAction,
  type GmailSearchMode,
  type GmailMessageSummary,
  type InboxDashboard,
  type PersonalDashboard,
} from './api'

type AssistantHomeProps = {
  desktopRuntime: boolean
  onAddProject: () => void
  onOpenProject: (path: string) => Promise<void>
  onError: (message: string) => void
  onNotice: (message: string) => void
}

type SpeechRecognitionLike = {
  lang: string
  continuous: boolean
  interimResults: boolean
  start: () => void
  stop: () => void
  onresult: ((event: { results: ArrayLike<{ 0: { transcript: string } }> }) => void) | null
  onerror: (() => void) | null
  onend: (() => void) | null
}

type SpeechRecognitionConstructor = new () => SpeechRecognitionLike

type ConversationTurn = {
  id: number
  request: string
  response: AssistantResponse
  createdAt?: string
}

function errorMessage(error: unknown) {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return 'Sharman could not complete that request.'
}

function formatSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

function formatWhen(value: string) {
  return new Date(value).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' })
}

export function AssistantHome({ desktopRuntime, onAddProject, onOpenProject, onError, onNotice }: AssistantHomeProps) {
  const [home, setHome] = useState<AssistantHomeSnapshot | null>(null)
  const [memory, setMemory] = useState<AssistantMemorySnapshot | null>(null)
  const [personal, setPersonal] = useState<PersonalDashboard | null>(null)
  const [inbox, setInbox] = useState<InboxDashboard | null>(null)
  const [audit, setAudit] = useState<AssistantAuditRecord[]>([])
  const [query, setQuery] = useState('')
  const [turns, setTurns] = useState<ConversationTurn[]>([])
  const [busy, setBusy] = useState(false)
  const [personalBusy, setPersonalBusy] = useState(false)
  const [completedTaskIds, setCompletedTaskIds] = useState<Set<string>>(new Set())
  const [cancelledReminderIds, setCancelledReminderIds] = useState<Set<string>>(new Set())
  const [aiSetupBusy, setAiSetupBusy] = useState<'runtime' | 'model' | null>(null)
  const [listening, setListening] = useState(false)
  const [memoryOpen, setMemoryOpen] = useState(false)
  const [inboxOpen, setInboxOpen] = useState(false)
  const [inboxBusy, setInboxBusy] = useState(false)
  const [historyOpen, setHistoryOpen] = useState(false)
  const [historySearch, setHistorySearch] = useState('')
  const [deviceOpen, setDeviceOpen] = useState(false)
  const [pendingRequest, setPendingRequest] = useState<string | null>(null)
  const [requestError, setRequestError] = useState<{ message: string; draft: string; completed: boolean } | null>(null)
  const [loadingHistory, setLoadingHistory] = useState(desktopRuntime)
  const [showLatest, setShowLatest] = useState(false)
  const [compactLayout, setCompactLayout] = useState(window.innerWidth <= 1040)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const conversationRef = useRef<HTMLElement>(null)
  const historySearchRef = useRef<HTMLInputElement>(null)
  const panelRef = useRef<HTMLElement>(null)
  const historyRef = useRef<HTMLElement>(null)
  const panelTriggerRef = useRef<HTMLButtonElement | null>(null)
  const requestInFlight = useRef(false)
  const personalWriteInFlight = useRef(false)
  const conversationLoad = useRef(0)
  const followLatest = useRef(true)
  const speechRef = useRef<SpeechRecognitionLike | null>(null)
  const localAiRef = useRef<HTMLElement>(null)
  const speechConstructor = useMemo(() => {
    const speechWindow = window as typeof window & {
      SpeechRecognition?: SpeechRecognitionConstructor
      webkitSpeechRecognition?: SpeechRecognitionConstructor
    }
    return speechWindow.SpeechRecognition || speechWindow.webkitSpeechRecognition || null
  }, [])

  useEffect(() => {
    if (!desktopRuntime) return
    let active = true
    void Promise.allSettled([getAssistantHome(), getAssistantMemory(), getPersonalDashboard(), getInboxDashboard(), getAssistantAudit()])
      .then(([homeResult, memoryResult, personalResult, inboxResult, auditResult]) => {
        if (!active) return
        if (homeResult.status === 'fulfilled') setHome(homeResult.value)
        if (memoryResult.status === 'fulfilled') { setMemory(memoryResult.value); setTurns(memoryResult.value.active.turns) }
        else setRequestError({ message: 'Saved conversations could not be loaded. You can safely reload them.', draft: '', completed: true })
        if (personalResult.status === 'fulfilled') setPersonal(personalResult.value)
        if (inboxResult.status === 'fulfilled') setInbox(inboxResult.value)
        if (auditResult.status === 'fulfilled') setAudit(auditResult.value)
        setLoadingHistory(false)
      })
    return () => { active = false }
  }, [desktopRuntime, onError])

  useEffect(() => {
    const list = conversationRef.current
    if (list && followLatest.current) list.scrollTo({ top: list.scrollHeight, behavior: 'instant' })
  }, [turns.length, memory?.active.id, pendingRequest])

  useEffect(() => {
    const input = inputRef.current
    if (input) { input.style.height = '0px'; input.style.height = `${Math.min(160, Math.max(52, input.scrollHeight))}px` }
  }, [query])

  useEffect(() => {
    if (memoryOpen || inboxOpen) panelRef.current?.focus()
  }, [memoryOpen, inboxOpen, compactLayout])

  useEffect(() => {
    if (historyOpen && compactLayout) historySearchRef.current?.focus()
  }, [historyOpen, compactLayout])

  useEffect(() => () => { speechRef.current?.stop() }, [])

  useEffect(() => {
    const media = window.matchMedia('(max-width: 1040px)')
    const update = () => setCompactLayout(media.matches)
    media.addEventListener('change', update)
    return () => media.removeEventListener('change', update)
  }, [])

  function closePanel() {
    setMemoryOpen(false); setInboxOpen(false)
    panelTriggerRef.current?.focus()
  }

  function scrollToLatest() {
    followLatest.current = true; setShowLatest(false)
    const list = conversationRef.current
    list?.scrollTo({ top: list.scrollHeight, behavior: 'instant' })
  }

  async function reloadConversation() {
    try {
      const snapshot = await getAssistantMemory(memory?.active.id ?? null)
      setMemory(snapshot); setTurns(snapshot.active.turns); setRequestError(null)
    } catch (error) { setRequestError((current) => ({ message: errorMessage(error), draft: current?.draft ?? '', completed: current?.completed ?? true })) }
  }

  async function copyReply(text: string) {
    try { await navigator.clipboard.writeText(text); onNotice('Reply copied.') }
    catch { onNotice('Clipboard is unavailable. You can select and copy the reply text instead.') }
  }

  const response = turns.at(-1)?.response ?? null

  async function runAction(action: AssistantAction) {
    if (action.requiresConfirmation && !window.confirm(`Allow Sharman to ${action.label.toLowerCase()}?\n\n${action.target}`)) return
    try {
      if (action.kind === 'setupLocalAi') {
        setDeviceOpen(true)
        window.requestAnimationFrame(() => localAiRef.current?.scrollIntoView({ behavior: 'smooth', block: 'center' }))
        return
      }
      if (action.kind === 'openProject') {
        await executeAssistantAction(action)
        await onOpenProject(action.target)
        return
      }
      await executeAssistantAction(action)
      onNotice(action.kind === 'playYoutube' ? 'YouTube opened with the best matching result.' : ['openUrl', 'openTrustedUrl'].includes(action.kind) ? 'Website opened in your default browser.' : 'Opened with the default Windows app.')
    } catch (error) {
      onError(errorMessage(error))
    }
  }

  async function runCommand(message: string) {
    const clean = message.trim()
    if (!clean || !desktopRuntime || requestInFlight.current || loadingHistory || !memory) return
    requestInFlight.current = true
    setRequestError(null)
    setPendingRequest(clean)
    setQuery('')
    followLatest.current = true
    setShowLatest(false)
    setBusy(true)
    let accepted: AssistantResponse | null = null
    try {
      const nextResponse = await askSharman(clean, memory?.active.id ?? null)
      accepted = nextResponse
      const nextMemory = await getAssistantMemory(nextResponse.conversationId)
      setMemory(nextMemory)
      setTurns(nextMemory.active.turns)
      const [nextPersonal, nextInbox, nextAudit] = await Promise.allSettled([getPersonalDashboard(), getInboxDashboard(), getAssistantAudit()])
      if (nextPersonal.status === 'fulfilled') setPersonal(nextPersonal.value)
      if (nextInbox.status === 'fulfilled') setInbox(nextInbox.value)
      if (nextAudit.status === 'fulfilled') setAudit(nextAudit.value)
      if ([nextPersonal, nextInbox, nextAudit].some((item) => item.status === 'rejected')) setRequestError({ message: 'Your reply is saved. Some connected data could not refresh; no tool action will be repeated automatically.', draft: '', completed: true })
      const automaticAction = nextResponse.actions.find((action) => ['playYoutube', 'openTrustedUrl'].includes(action.kind) && !action.requiresConfirmation)
      if (automaticAction) await runAction(automaticAction)
    } catch (error) {
      setRequestError({ message: accepted ? 'Your request completed, but the saved conversation could not refresh. Reload it before repeating any action.' : 'The request could not be confirmed. Reload the conversation or check Inbox activity before sending it again.', draft: clean, completed: Boolean(accepted) })
    } finally {
      requestInFlight.current = false
      setPendingRequest(null)
      setBusy(false)
      window.requestAnimationFrame(() => inputRef.current?.focus())
    }
  }

  function submit(event: FormEvent) {
    event.preventDefault()
    void runCommand(query)
  }

  async function selectConversation(id: string) {
    if (requestInFlight.current) return
    const version = ++conversationLoad.current
    setLoadingHistory(true)
    try {
      const snapshot = await getAssistantMemory(id)
      if (version !== conversationLoad.current) return
      followLatest.current = true
      setMemory(snapshot)
      setTurns(snapshot.active.turns)
      setRequestError(null); setHistoryOpen(false); setShowLatest(false)
      inputRef.current?.focus()
    } catch (error) {
      if (version === conversationLoad.current) onError(errorMessage(error))
    } finally {
      if (version === conversationLoad.current) { setLoadingHistory(false); window.requestAnimationFrame(() => inputRef.current?.focus()) }
    }
  }

  async function startNewChat() {
    if (requestInFlight.current) return
    if (query.trim() && !window.confirm('Start a new chat and clear your unsent message? Your saved conversations will stay.')) return
    try {
      const snapshot = await createAssistantConversation()
      setMemory(snapshot)
      setTurns([])
      setQuery('')
      setRequestError(null); setHistoryOpen(false); setShowLatest(false)
      followLatest.current = true
      window.setTimeout(() => inputRef.current?.focus(), 0)
    } catch (error) {
      onError(errorMessage(error))
    }
  }

  async function forgetMemory(memoryId: number | null) {
    if (memoryId === null && !window.confirm('Forget every personal fact saved by Sharman?\n\nChat history will remain, but long-term memory will be cleared.')) return
    try {
      const snapshot = await forgetAssistantMemory(memoryId)
      const current = await getAssistantMemory(memory?.active.id ?? snapshot.active.id)
      setMemory(current)
      onNotice(memoryId === null ? 'Long-term memory cleared.' : 'Memory removed.')
    } catch (error) {
      onError(errorMessage(error))
    }
  }

  async function completePersonalTask(taskId: string, title: string) {
    if (personalWriteInFlight.current || !window.confirm(`Complete “${title}”?\n\nIts pending reminders will also be cancelled. Other tasks will not change.`)) return
    personalWriteInFlight.current = true
    setPersonalBusy(true)
    try {
      const dashboard = await setPersonalTaskCompleted(taskId, true)
      setPersonal(dashboard)
      setCompletedTaskIds((current) => new Set([...current, taskId]))
      // Keep historical responses intact; only update this view's live controls.
      const linkedIds = turns.flatMap((turn) => turn.response.reminders.filter((item) => item.taskId === taskId).map((item) => item.id))
      setCancelledReminderIds((current) => new Set([...current, ...linkedIds]))
      onNotice('Task completed.')
    } catch (error) {
      onError(errorMessage(error))
    } finally {
      personalWriteInFlight.current = false
      setPersonalBusy(false)
    }
  }

  async function dismissReminder(reminderId: string, title: string) {
    if (personalWriteInFlight.current || !window.confirm(`Cancel the reminder “${title}”?\n\nThe task itself will stay open. No other reminder will change.`)) return
    personalWriteInFlight.current = true
    setPersonalBusy(true)
    try {
      setPersonal(await dismissPersonalReminder(reminderId))
      setCancelledReminderIds((current) => new Set([...current, reminderId]))
      onNotice('Reminder dismissed.')
    } catch (error) {
      onError(errorMessage(error))
    } finally {
      personalWriteInFlight.current = false
      setPersonalBusy(false)
    }
  }

  async function connectInbox() {
    if (!window.confirm('Connect Gmail with read-only access?\n\nSharman can search and read message metadata, but cannot send, draft, modify, or delete email. Your refresh token is stored in Windows Credential Manager.')) return
    setInboxBusy(true)
    try {
      await connectGmail()
      const [nextInbox, nextAudit] = await Promise.all([getInboxDashboard(), getAssistantAudit()])
      setInbox(nextInbox)
      setAudit(nextAudit)
      onNotice('Gmail connected with read-only access.')
    } catch (error) {
      onError(errorMessage(error))
    } finally {
      setInboxBusy(false)
    }
  }

  async function disconnectInbox() {
    if (!window.confirm('Disconnect Gmail and remove its OAuth credential from Windows Credential Manager?\n\nPreviously extracted local action items will remain until dismissed.')) return
    try {
      await disconnectGmail()
      setInbox(await getInboxDashboard())
      setAudit(await getAssistantAudit())
      onNotice('Gmail disconnected.')
    } catch (error) {
      onError(errorMessage(error))
    }
  }

  async function refreshInbox(mode: GmailSearchMode = 'needsAction') {
    setInboxBusy(true)
    try {
      const nextInbox = await refreshGmailInbox(mode)
      setInbox(nextInbox)
      setAudit(await getAssistantAudit())
      onNotice(`Inbox checked. ${nextInbox.needsAction.length} actionable item(s) found.`)
    } catch (error) {
      onError(errorMessage(error))
    } finally {
      setInboxBusy(false)
    }
  }

  async function dismissInboxAction(actionId: string) {
    try {
      setInbox(await dismissEmailAction(actionId))
      setAudit(await getAssistantAudit())
    } catch (error) {
      onError(errorMessage(error))
    }
  }

  async function addEmailReminder(actionId: string) {
    try {
      setPersonal(await createEmailReminder(actionId))
      setInbox(await getInboxDashboard())
      setAudit(await getAssistantAudit())
      onNotice('Reminder created for two hours before the verified deadline.')
    } catch (error) {
      onError(errorMessage(error))
    }
  }

  async function openEmail(action: EmailAction | GmailMessageSummary) {
    try {
      await openGmailMessage(action.threadId)
      setAudit(await getAssistantAudit())
    } catch (error) {
      onError(errorMessage(error))
    }
  }

  async function refreshHome() {
    setHome(await getAssistantHome())
  }

  async function installRuntime() {
    if (!home || !window.confirm('Install the official Ollama local-AI runtime using Windows Package Manager?\n\nThis downloads and installs software on this computer. It does not download the AI model yet.')) return
    setAiSetupBusy('runtime')
    try {
      await installLocalAiRuntime()
      await refreshHome()
      onNotice('Local AI runtime installed. Download the recommended model to finish setup.')
    } catch (error) {
      onError(errorMessage(error))
    } finally {
      setAiSetupBusy(null)
    }
  }

  async function downloadModel() {
    if (!home || !window.confirm(`Download ${home.device.recommendedModelName} for local reasoning?\n\nApproximate download: ${home.device.recommendedDownloadGb} GB. The model stays on this computer.`)) return
    setAiSetupBusy('model')
    try {
      await pullRecommendedLocalModel()
      await refreshHome()
      onNotice(`${home.device.recommendedModelName} is ready. Sharman can now answer general requests locally.`)
    } catch (error) {
      onError(errorMessage(error))
    } finally {
      setAiSetupBusy(null)
    }
  }

  function startVoice() {
    if (listening) { speechRef.current?.stop(); setListening(false); return }
    if (!speechConstructor) {
      onNotice('Voice input is unavailable in this Windows webview. Typed commands still use the same assistant pipeline.')
      return
    }
    const recognition = new speechConstructor()
    speechRef.current = recognition
    recognition.lang = 'en-IN'
    recognition.continuous = false
    recognition.interimResults = false
    recognition.onresult = (event) => {
      const transcript = event.results[0]?.[0]?.transcript?.trim()
      if (transcript) {
        setQuery(transcript)
        window.setTimeout(() => inputRef.current?.focus(), 0)
      }
    }
    recognition.onerror = () => { setListening(false); onNotice('I could not hear that. You can type the command instead.') }
    recognition.onend = () => setListening(false)
    setListening(true)
    try { recognition.start() } catch { setListening(false); onNotice('Voice could not start. Please type your message instead.') }
  }

  const suggestions = response?.suggestions.length
    ? response.suggestions
    : ['Show my unread emails', 'Show my important emails', 'Find internship and OA emails', 'What tasks do I have today?']

  const hasConversation = turns.length > 0 || pendingRequest !== null
  const panelOpen = memoryOpen || inboxOpen
  const matchingConversations = memory?.conversations.filter((item) => item.title.toLowerCase().includes(historySearch.toLowerCase())) ?? []

  return <section className={`assistant-home native-assistant ${hasConversation ? 'chat-active' : ''}`} onKeyDown={(event) => {
    if (event.key === 'Escape') {
      if (panelOpen) { event.stopPropagation(); closePanel() }
      else if (historyOpen) { setHistoryOpen(false); inputRef.current?.focus() }
    }
    if (event.ctrlKey && event.key.toLowerCase() === 'k') {
      event.preventDefault(); setMemoryOpen(false); setInboxOpen(false); setHistoryOpen(true)
      window.requestAnimationFrame(() => historySearchRef.current?.focus())
    }
    const region = compactLayout ? panelOpen ? panelRef.current : historyOpen ? historyRef.current : null : null
    if (region && event.key === 'Tab') {
      const controls = Array.from(region.querySelectorAll<HTMLElement>('button:not([disabled]),input:not([disabled]),a[href],textarea:not([disabled]),select:not([disabled]),[tabindex="0"]')).filter((item) => item.getClientRects().length)
      const first = controls[0], last = controls.at(-1)
      if (event.shiftKey && (document.activeElement === first || document.activeElement === region)) { event.preventDefault(); last?.focus() }
      else if (!event.shiftKey && (document.activeElement === last || document.activeElement === region)) { event.preventDefault(); first?.focus() }
    }
  }}>
    <div className={`assistant-workspace ${historyOpen ? 'history-open' : ''}`}>
      {historyOpen && compactLayout && <button type="button" className="chat-history-backdrop" aria-label="Close conversation list" onClick={() => { setHistoryOpen(false); inputRef.current?.focus() }} />}
      <aside className="chat-history" id="chat-history" ref={historyRef} aria-label="Conversations" role={compactLayout ? 'dialog' : undefined} aria-modal={compactLayout && historyOpen ? true : undefined}>
        <div className="chat-history-heading"><span>YOUR WORKSPACE</span><button className="chat-icon-button history-close" type="button" aria-label="Close conversations" onClick={() => { setHistoryOpen(false); inputRef.current?.focus() }}><ChatIcon name="close" /></button></div>
        <button type="button" className="chat-new-button" disabled={busy || loadingHistory || !desktopRuntime} onClick={() => void startNewChat()}><ChatIcon name="plus" /> New conversation</button>
        <label className="chat-history-search"><ChatIcon name="search" /><input ref={historySearchRef} aria-label="Search conversations" placeholder="Search chats" value={historySearch} onChange={(event) => setHistorySearch(event.target.value)} /><kbd>Ctrl K</kbd></label>
        <h2>Recent conversations</h2>
        <nav aria-label="Saved conversations">{loadingHistory ? <p className="chat-history-empty">Loading saved chats…</p> : matchingConversations.length ? matchingConversations.map((conversation) => <button type="button" key={conversation.id} className={conversation.id === memory?.active.id ? 'selected' : ''} aria-current={conversation.id === memory?.active.id ? 'page' : undefined} disabled={busy} onClick={() => void selectConversation(conversation.id)}><ChatIcon name="chat" /><span><strong>{conversation.title}</strong><small>{formatWhen(conversation.updatedAt)}</small></span></button>) : <p className="chat-history-empty">{historySearch ? 'No conversations match your search.' : 'Your conversations will appear here.'}</p>}</nav>
        <div className="chat-history-footer"><ChatIcon name="shield" /><div><strong>Saved on this device</strong><p>Chat history and chosen memories stay in your local database.</p></div></div>
        <button type="button" className="chat-device-link" onClick={() => { setDeviceOpen(true); setHistoryOpen(false); window.requestAnimationFrame(() => localAiRef.current?.scrollIntoView({ behavior: 'smooth', block: 'center' })) }}>Local AI & projects <span>↗</span></button>
      </aside>
    <div className={`assistant-chat-shell ${panelOpen ? 'context-open' : ''}`}>
      <header className="assistant-chat-toolbar">
        <button type="button" className="chat-icon-button history-toggle" aria-label="Show conversations" aria-controls="chat-history" aria-expanded={historyOpen} onClick={() => { setHistoryOpen((current) => !current); setMemoryOpen(false); setInboxOpen(false) }}><ChatIcon name="panel" /></button>
        <div className="assistant-identity"><span>S</span><div><h1>{memory?.active.title || 'Your personal assistant'}</h1><small>{home?.device.localModelInstalled ? <><i /> {home.device.localModelName} · on-device</> : desktopRuntime ? 'Controlled tools · local memory' : 'Windows desktop required for tools'}</small></div></div>
        <div className="assistant-chat-controls">
          <button type="button" className={`memory-button ${inboxOpen ? 'active' : ''}`} aria-expanded={inboxOpen} aria-controls={inboxOpen ? 'assistant-inbox-panel' : undefined} onClick={(event) => { panelTriggerRef.current = event.currentTarget; setInboxOpen((current) => !current); setMemoryOpen(false) }}><ChatIcon name="inbox" /> Inbox <b>{inbox?.needsAction.length ?? 0}</b></button>
          <button type="button" className={`memory-button ${memoryOpen ? 'active' : ''}`} aria-expanded={memoryOpen} aria-controls={memoryOpen ? 'assistant-memory-panel' : undefined} onClick={(event) => { panelTriggerRef.current = event.currentTarget; setMemoryOpen((current) => !current); setInboxOpen(false) }}><ChatIcon name="memory" /> Memory <b>{memory?.memories.length ?? 0}</b></button>
        </div>
      </header>

      {!hasConversation ? <div className="assistant-welcome">
        <div className="welcome-mark" aria-hidden="true">S<span /></div>
        <p className="eyebrow">YOUR DAY, A LITTLE LIGHTER</p>
        <h2>What can we get done?</h2>
        <p>Find what you need, pick up where you left off, or just think out loud. You stay in control of every action.</p>
        <div className="chat-starters">{[
          { icon: 'inbox' as const, title: 'Check my inbox', detail: 'See recent, unread email', prompt: 'Show my unread emails' },
          { icon: 'clock' as const, title: 'Plan my day', detail: 'Review your saved tasks', prompt: 'What tasks do I have today?' },
          { icon: 'file' as const, title: 'Find a file', detail: 'Look in recent downloads', prompt: 'Find files downloaded yesterday' },
          { icon: 'memory' as const, title: 'Remember a preference', detail: 'Save only what you choose', prompt: 'Remember that I prefer concise answers.' },
          { icon: 'clock' as const, title: 'Set a reminder', detail: 'Use minutes, hours, or days', prompt: 'Remind me in 20 minutes to take a break' },
          { icon: 'clock' as const, title: 'Review overdue tasks', detail: 'See deadlines that have passed', prompt: 'Show my overdue tasks' },
        ].map((item) => <button key={item.title} type="button" disabled={busy || loadingHistory || !desktopRuntime || !memory} onClick={() => { setQuery(item.prompt); inputRef.current?.focus() }}><ChatIcon name={item.icon} /><strong>{item.title}</strong><span>{item.detail}</span><small>Start a message ↗</small></button>)}</div>
      </div> : <section className="assistant-conversation" aria-label="Current conversation" ref={conversationRef} onScroll={(event) => {
        const list = event.currentTarget
        followLatest.current = list.scrollHeight - list.scrollTop - list.clientHeight < 100
        setShowLatest(!followLatest.current)
      }}>
        <div className="conversation-date"><span>Saved conversation · only on this device</span></div>
        {turns.map((turn) => <div className="conversation-turn" key={turn.id}>
          <div className="message-row user-row"><div className="chat-avatar">You</div><div className="user-message"><p>{turn.request}</p></div></div>
          <div className="message-row assistant-row"><div className="chat-avatar sharman-avatar">S</div><article className="assistant-answer">
            <header><div><span>SHARMAN</span><h2>{turn.response.title}</h2></div><b>{turn.response.localOnly ? 'On-device' : /gmail|email/.test(turn.response.intent) ? 'Gmail · read only' : 'Connected tool'}</b></header>
            <div className="assistant-response-text"><MessageText text={turn.response.answer} /></div>
            {turn.response.planSteps.length > 0 && <details className="assistant-tool-trace" open={turn.response.planSteps.some((step) => /failed|denied|unavailable/.test(step.status))}><summary><ChatIcon name="shield" /><span>Tool activity <small>{turn.response.planSteps.filter((step) => step.status === 'completed').length}/{turn.response.planSteps.length} completed</small></span><span>{turn.response.planSteps.every((step) => step.permissionLevel === 0) ? 'Read only · L0' : 'Permission checked'}</span></summary><div className="assistant-plan-steps">{turn.response.planSteps.map((step, index) => <div key={`${step.tool}-${index}`}><span>{index + 1}</span><div><strong>{step.tool}</strong><small>{step.summary}</small></div><b className={step.status}>{step.status} · L{step.permissionLevel}</b></div>)}</div></details>}
            {turn.response.personalTasks.length > 0 && <div className="assistant-personal-results">{turn.response.personalTasks.map((task) => <article key={task.id}><div><strong>{task.title}</strong><small>{task.dueAt ? formatWhen(task.dueAt) : 'No deadline'} · {task.source}</small></div>{task.status === 'completed' || completedTaskIds.has(task.id) ? <span className="personal-state">Completed</span> : <button type="button" className="personal-confirm" disabled={personalBusy} aria-label={`Complete task: ${task.title}`} onClick={() => void completePersonalTask(task.id, task.title)}>Complete task</button>}</article>)}</div>}
            {turn.response.reminders.length > 0 && <div className="assistant-reminder-results">{turn.response.reminders.map((reminder) => <article key={reminder.id}><div><strong>{reminder.title}</strong><small>{formatWhen(reminder.remindAt)}</small></div>{cancelledReminderIds.has(reminder.id) ? <span className="personal-state">Cancelled</span> : reminder.status !== 'pending' ? <span className="personal-state">{reminder.status}</span> : <button type="button" className="personal-confirm" disabled={personalBusy} aria-label={`Cancel reminder: ${reminder.title}`} onClick={() => void dismissReminder(reminder.id, reminder.title)}>Cancel reminder</button>}</article>)}</div>}
            {turn.response.emailActions.length > 0 && <div className="assistant-email-results">{turn.response.emailActions.map((item) => <article key={item.id}>
              <header><span>{item.actionType}</span><b>{Math.round(item.confidence * 100)}% confidence</b></header>
              <strong>{item.title}</strong>
              <small>{item.company || item.sender}{item.deadline ? ` · due ${formatWhen(item.deadline)}` : ''}</small>
              <p>{item.evidence || 'No bounded snippet was available.'}</p>
              <footer><button type="button" onClick={() => void openEmail(item)}>Open email</button>{item.deadline && item.confidence >= .85 && !item.reminderCreated && <button type="button" onClick={() => void addEmailReminder(item.id)}>Remind 2h before</button>}<button type="button" onClick={() => void dismissInboxAction(item.id)}>Ignore</button></footer>
            </article>)}</div>}
            {turn.response.emailMessages.length > 0 && <div className="assistant-email-results">{turn.response.emailMessages.map((item) => <article key={item.id}>
              <header><span>{turn.response.intent === 'gmail_recent' ? 'recent' : 'unread'}</span><b>GMAIL · READ ONLY</b></header>
              <strong>{item.subject}</strong>
              <small>{item.sender} · {formatWhen(item.receivedAt)}</small>
              <p>{item.snippet || 'No bounded snippet was available.'}</p>
              <footer><button type="button" onClick={() => void openEmail(item)}>Open email</button></footer>
            </article>)}</div>}
            {turn.response.projects.length > 0 && <div className="assistant-project-results">{turn.response.projects.map((project) => <div key={project.id}><div><strong>{project.name}</strong><span>{project.totalTasks ? `${project.completedTasks}/${project.totalTasks} tasks` : 'No task plan yet'} · {project.blockers} blockers</span></div><b>{project.totalTasks ? `${project.progressPercent}%` : 'Unplanned'}</b></div>)}</div>}
            {turn.response.files.length > 0 && <div className="assistant-file-results">{turn.response.files.map((file, index) => {
              const action = turn.response.actions[index]
              return <div key={file.path}><span className="file-result-icon">{file.kind.slice(0, 4)}</span><div><strong>{file.name}</strong><small>{file.path}</small><em>{formatSize(file.sizeBytes)} · {new Date(file.modifiedAt).toLocaleString()}</em></div>{action && <button type="button" onClick={() => void runAction(action)}>{action.label}</button>}</div>
            })}</div>}
            {turn.response.files.length === 0 && turn.response.actions.length > 0 && <div className="assistant-actions">{turn.response.actions.map((action) => <button className="primary-button" type="button" key={action.id} onClick={() => void runAction(action)}>{action.label}</button>)}</div>}
            <footer className="chat-message-footer">{turn.createdAt && <time dateTime={turn.createdAt}>{new Date(turn.createdAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</time>}<button type="button" onClick={() => void copyReply(turn.response.answer)} aria-label="Copy reply"><ChatIcon name="copy" /> Copy</button></footer>
          </article></div>
        </div>)}
        {pendingRequest && <div className="message-row user-row pending-user-message"><div className="chat-avatar">You</div><div className="user-message"><p>{pendingRequest}</p></div></div>}
        {busy && <div className="message-row assistant-row thinking-row" role="status"><div className="chat-avatar sharman-avatar">S</div><div className="thinking-bubble"><span>Working on your request</span><i /><i /><i /></div></div>}
      </section>}

      {panelOpen && compactLayout && <button type="button" className="chat-panel-backdrop" aria-label="Close side panel" onClick={closePanel} />}
      {memoryOpen && <aside className="assistant-memory-drawer" id="assistant-memory-panel" ref={panelRef} tabIndex={-1} role={compactLayout ? 'dialog' : 'region'} aria-modal={compactLayout ? true : undefined} aria-label="Local memory">
        <header><div><strong>Local memory</strong><small>Only facts you explicitly ask Sharman to remember</small></div><button className="chat-icon-button" type="button" onClick={closePanel} aria-label="Close memory"><ChatIcon name="close" /></button></header>
        {Boolean(memory?.memories.length) && <button className="clear-memory-button" type="button" onClick={() => void forgetMemory(null)}>Clear saved facts</button>}
        {memory?.memories.length ? <div>{memory.memories.map((item) => <article key={item.id}><p>{item.content}</p><button type="button" aria-label={`Forget ${item.content}`} onClick={() => void forgetMemory(item.id)}>Forget</button></article>)}</div> : <p className="memory-empty">No saved facts. Try: “Remember that I prefer concise answers.”</p>}
        <footer>Stored in <code>sharman-memory.db</code> on this computer. Passwords, tokens, and API keys are rejected.</footer>
      </aside>}

      {inboxOpen && <aside className="assistant-memory-drawer action-inbox-drawer" id="assistant-inbox-panel" ref={panelRef} tabIndex={-1} role={compactLayout ? 'dialog' : 'region'} aria-modal={compactLayout ? true : undefined} aria-label="Action Inbox">
        <header><div><strong>Action Inbox</strong><small>{inbox?.status.connected ? `${inbox.status.accountEmail || 'Gmail'} · read only` : 'Gmail is not connected'}</small></div><button className="chat-icon-button" type="button" onClick={closePanel} aria-label="Close inbox"><ChatIcon name="close" /></button></header>
        {inbox?.status.connected ? <div className="inbox-header-actions"><button type="button" disabled={inboxBusy} onClick={() => void refreshInbox()}>{inboxBusy ? 'Checking…' : 'Check now'}</button><button type="button" disabled={inboxBusy} onClick={() => void disconnectInbox()}>Disconnect</button></div> : <button className="connect-gmail-button" type="button" disabled={inboxBusy || !desktopRuntime} onClick={() => void connectInbox()}>{inboxBusy ? 'Waiting for Google…' : 'Connect Gmail'}</button>}
        {inbox?.scan && <div className="inbox-scan-receipt"><span><strong>{inbox.scan.scanned}</strong> scanned last check</span><span><strong>{inbox.scan.matched}</strong> matched action rules</span></div>}
        {!inbox?.status.connected ? <section className="gmail-setup"><strong>Read-only Inbox Intelligence</strong><p>{inbox?.status.setupHint || 'Choose a Google Desktop OAuth client JSON file to connect.'}</p><small>Refresh tokens go to Windows Credential Manager—not SQLite. Sharman cannot send, draft, modify, or delete email.</small></section> : <>
          <div className={`gmail-sync-state ${inbox.status.sync.lastError ? 'error' : ''}`}><span>{inbox.status.sync.lastError ? 'Sync needs attention' : inbox.status.sync.lastHistoryId ? 'Incremental sync ready' : 'First sync pending'}</span><small>{inbox.status.sync.lastError || (inbox.status.sync.lastSuccessfulSync ? `Last successful check ${formatWhen(inbox.status.sync.lastSuccessfulSync)}` : 'SHARMAN will save a Gmail history checkpoint after a successful check.')}</small></div>
          <nav className="inbox-filters"><button type="button" disabled={inboxBusy} onClick={() => void refreshInbox('important')}>Important</button><button type="button" disabled={inboxBusy} onClick={() => void refreshInbox('needsAction')}>Needs action</button><button type="button" disabled={inboxBusy} onClick={() => void refreshInbox('internship')}>Internship / OA</button></nav>
          <section><h3>Needs Action</h3>{inbox.needsAction.length ? inbox.needsAction.map((item) => <article className="inbox-action-card" key={item.id}><div><p>{item.title}</p><small>{item.company || item.sender} · {Math.round(item.confidence * 100)}% confidence</small>{item.deadline && <em>Deadline {formatWhen(item.deadline)}</em>}</div><div><button type="button" onClick={() => void openEmail(item)}>Open</button>{item.deadline && item.confidence >= .85 && !item.reminderCreated && <button type="button" onClick={() => void addEmailReminder(item.id)}>Remind</button>}<button type="button" onClick={() => void dismissInboxAction(item.id)}>Ignore</button></div></article>) : <p className="memory-empty">No extracted action items. Choose a search above to check Gmail.</p>}</section>
          <section><h3>Upcoming Deadlines</h3>{inbox.upcomingDeadlines.length ? inbox.upcomingDeadlines.map((item) => <article key={item.id}><div><p>{item.title}</p><small>{item.deadline ? formatWhen(item.deadline) : 'Date requires review'}</small></div><b>{item.reminderCreated ? 'Reminder set' : `${Math.round(item.confidence * 100)}%`}</b></article>) : <p className="memory-empty">No extracted deadlines.</p>}</section>
          <section><h3>Suggested Reminders</h3>{inbox.upcomingDeadlines.filter((item) => item.confidence >= .85 && !item.reminderCreated).length ? inbox.upcomingDeadlines.filter((item) => item.confidence >= .85 && !item.reminderCreated).map((item) => <article key={item.id}><div><p>{item.title}</p><small>Two hours before {item.deadline ? formatWhen(item.deadline) : 'the deadline'}</small></div><button type="button" onClick={() => void addEmailReminder(item.id)}>Create reminder</button></article>) : <p className="memory-empty">No high-confidence reminder suggestions.</p>}</section>
          <section><h3>Dismissed</h3>{inbox.dismissed.length ? inbox.dismissed.slice(0, 5).map((item) => <article key={item.id}><div><p>{item.title}</p><small>{item.company || item.sender}</small></div><b>Ignored</b></article>) : <p className="memory-empty">No dismissed email actions.</p>}</section>
        </>}
        <section><h3>Today’s Tasks</h3>{personal?.tasks.length ? personal.tasks.map((task) => <article key={task.id}><button className="task-check" type="button" disabled={personalBusy} aria-label={`Complete ${task.title}`} onClick={() => void completePersonalTask(task.id, task.title)}>✓</button><div><p>{task.title}</p><small>{task.dueAt ? formatWhen(task.dueAt) : 'No deadline'}</small></div></article>) : <p className="memory-empty">No incomplete tasks due today.</p>}</section>
        <section><h3>Reminders</h3>{personal?.reminders.length ? personal.reminders.map((reminder) => <article key={reminder.id}><div><p>{reminder.title}</p><small>{formatWhen(reminder.remindAt)}</small></div><button type="button" disabled={personalBusy} onClick={() => void dismissReminder(reminder.id, reminder.title)}>Cancel reminder</button></article>) : <p className="memory-empty">No upcoming reminders.</p>}</section>
        <section className="assistant-audit"><h3>Recent Execution</h3>{audit.length ? audit.slice(0, 12).map((item) => <article key={item.id}><div><p>{item.tool}</p><small>{item.actor} · {item.permission} · {item.outcome}</small><em>{item.summary}</em></div><time>{new Date(item.occurredAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</time></article>) : <p className="memory-empty">No tool executions recorded yet.</p>}</section>
        <footer>Email bodies and OAuth tokens are excluded from project context. Only bounded extracted metadata is cached locally. The reminder scheduler continues while Sharman runs in the tray.</footer>
      </aside>}

      <div className="assistant-suggestions" aria-label="Suggested commands">{suggestions.slice(0, 3).map((suggestion) => <button type="button" key={suggestion} disabled={busy || loadingHistory || !desktopRuntime || !memory} onClick={() => { setQuery(suggestion); inputRef.current?.focus() }}>{suggestion} <span>↗</span></button>)}</div>

      <form className="assistant-composer" onSubmit={submit}>
        {showLatest && <button type="button" className="chat-latest-button" onClick={scrollToLatest}><ChatIcon name="down" /> Latest message</button>}
        {requestError && <div className="chat-request-error" role="alert"><p>{requestError.message}</p><div><button type="button" disabled={busy} onClick={() => void reloadConversation()}>Reload conversation</button>{requestError.draft && !requestError.completed && <button type="button" onClick={() => { setQuery(requestError.draft); inputRef.current?.focus() }}>Restore message</button>}</div><small>No action is retried automatically.</small></div>}
        <div className="chat-composer-input"><textarea ref={inputRef} value={query} onChange={(event) => setQuery(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); if (!event.repeat) void runCommand(query) } }} placeholder={loadingHistory ? 'Loading your conversations…' : 'Message Sharman…'} aria-label="Ask Sharman" aria-describedby="chat-composer-help" rows={1} disabled={!desktopRuntime || busy || loadingHistory || !memory} /><div className="chat-composer-buttons"><button className={`voice-button ${listening ? 'listening' : ''}`} type="button" disabled={!speechConstructor || !desktopRuntime || busy} onClick={startVoice} aria-label={listening ? 'Stop voice input' : 'Speak to Sharman'} title={speechConstructor ? 'Speak to Sharman' : 'Voice is not available in this webview'}><ChatIcon name="mic" /></button><button className="assistant-send" disabled={!desktopRuntime || busy || loadingHistory || !memory || !query.trim()} aria-label="Send message">{busy ? <span className="chat-sending-spinner" /> : <ChatIcon name="up" />}</button></div></div>
        <div className="chat-composer-footer" id="chat-composer-help"><span><ChatIcon name="shield" /> {desktopRuntime ? 'Local memory · permission-checked tools' : 'Open the Windows app to use tools'}</span><small>Enter to send <span>· Shift + Enter for a new line</span></small></div>
      </form>
    </div>

    </div>
    <details className="assistant-device-details" open={deviceOpen} onToggle={(event) => setDeviceOpen(event.currentTarget.open)}><summary>Local AI & project workspace <span>Device setup and optional coding tools</span></summary>
      <div className="assistant-home-grid">
        <section className="assistant-panel assistant-command-center"><header><div><p className="section-label">QUICK ACTIONS</p><h2>Start with a command</h2></div></header><div className="command-skill-grid">
          <button type="button" onClick={() => void runCommand('Open LeetCode')}><span>↗</span><div><strong>Open LeetCode</strong><small>Use the trusted browser tool</small></div></button>
          <button type="button" onClick={() => void runCommand('Open today’s LeetCode')}><span>LC</span><div><strong>Today’s challenge</strong><small>Resolve and open the daily problem</small></div></button>
          <button type="button" onClick={() => void runCommand('Remind me at 8 PM to solve it')}><span>R</span><div><strong>Set a reminder</strong><small>Create a task and notification</small></div></button>
          <button type="button" onClick={() => void runCommand('What tasks do I have today?')}><span>T</span><div><strong>Today’s tasks</strong><small>Read the local personal task list</small></div></button>
          <button type="button" onClick={() => void runCommand('Show my important emails')}><span>@</span><div><strong>Important email</strong><small>Read-only Gmail action extraction</small></div></button>
          <button type="button" onClick={() => void runCommand('Find internship and OA emails')}><span>OA</span><div><strong>Internship inbox</strong><small>Find assessments and deadlines</small></div></button>
        </div></section>

        <section className="assistant-panel local-model-panel" ref={localAiRef}><header><div><p className="section-label">LOCAL AI</p><h2>{home?.device.localModelName || home?.device.recommendedModelName || 'Inspecting device…'}</h2></div><span className={`model-status ${home?.device.localModelInstalled ? 'ready' : ''}`}>{home?.device.localModelInstalled ? 'Ready' : home?.device.localRuntimeInstalled ? 'Model needed' : 'Runtime needed'}</span></header>{home && <>
          <p>{home.device.localModelInstalled ? `Private reasoning is connected to ${home.device.localModelName} on localhost.` : home.device.recommendation}</p>
          <dl><div><dt>Memory</dt><dd>{home.device.totalMemoryGb} GB</dd></div><div><dt>Model</dt><dd>{home.device.recommendedModelSize}</dd></div><div><dt>Download</dt><dd>~{home.device.recommendedDownloadGb} GB</dd></div></dl>
          {!home.device.localRuntimeInstalled && <div className="local-ai-actions"><button className="primary-button" type="button" disabled={aiSetupBusy !== null} onClick={() => void installRuntime()}>{aiSetupBusy === 'runtime' ? 'Installing…' : 'Install local runtime'}</button><button className="ghost-button" type="button" onClick={() => void runAction({ id: 'ollama-download', label: 'Open official download', kind: 'openUrl', target: 'https://ollama.com/download/windows', requiresConfirmation: false })}>Manual download</button></div>}
          {home.device.localRuntimeInstalled && !home.device.localModelInstalled && <div className="local-ai-actions"><button className="primary-button" type="button" disabled={aiSetupBusy !== null} onClick={() => void downloadModel()}>{aiSetupBusy === 'model' ? `Downloading ${home.device.recommendedDownloadGb} GB…` : `Download ${home.device.recommendedModelName}`}</button></div>}
          <small>{home.device.localModelInstalled ? 'Conversation context and saved memories are supplied only to this local model.' : 'Nothing downloads automatically. Each installation step requires confirmation.'}</small>
        </>}</section>
      </div>

      {home?.projects.length ? <section className="assistant-panel project-portfolio portfolio-wide"><header><div><p className="section-label">PROJECT MEMORY</p><h2>Your tracked projects</h2></div><button type="button" onClick={onAddProject}>+ Add project</button></header><div className="portfolio-compact-grid">{home.projects.map((project) => <button type="button" className="portfolio-card" key={project.id} onClick={() => void onOpenProject(project.path)}><div className="portfolio-card-heading"><div><strong>{project.name}</strong><span>{project.branch} · {project.activeAgents} active</span></div><b>{project.totalTasks ? `${project.progressPercent}%` : 'Not planned'}</b></div><div className="progress-track"><span style={{ width: `${project.totalTasks ? project.progressPercent : 0}%` }} /></div><div className="portfolio-meta"><span>{project.totalTasks ? `${project.completedTasks}/${project.totalTasks} tasks done` : 'Add tasks for measurable progress'}</span><span className={project.blockers ? 'has-blockers' : ''}>{project.blockers ? `${project.blockers} blockers` : 'No blockers'}</span></div></button>)}</div></section> : <section className="assistant-project-invite"><div><p className="section-label">OPTIONAL PROJECT SKILL</p><strong>Need coding-agent coordination?</strong><span>Add a project for AgentRelay memory, task progress, and handoffs.</span></div><button type="button" onClick={onAddProject}>Add project</button></section>}
    </details>
  </section>
}
