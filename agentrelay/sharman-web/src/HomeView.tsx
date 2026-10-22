import { AnimatePresence, motion, useReducedMotion } from 'framer-motion';
import {
  AlertTriangle,
  BellRing,
  Check,
  CheckCircle2,
  Cloud,
  Clock3,
  House,
  LockKeyhole,
  PackageCheck,
  Radio,
  RotateCcw,
  ShieldAlert,
  Sparkles,
  UserRoundCheck,
  Video,
  Volume2,
  X,
} from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { acceptHomeSuggestion, checkRingPermission, explainHomeWithBedrock, getBedrockStatus, getHome, getRingPlaygroundDevices, getRingPlaygroundDevice, getRingStatus, ingestRingTestEvent, resetHomeDemo, simulateHome, syncRingPlayground } from './api';
import type { BedrockStatus, CareExplanation, HomeScenario, HomeState, HomeWorkflow, RingStatus, RingPlaygroundDevice, RingPlaygroundSnapshot } from './types';
import RingPlaygroundPanel from './RingPlaygroundPanel';
import './home.css';

const scenarios: Array<{
  id: HomeScenario;
  label: string;
  detail: string;
  Icon: typeof PackageCheck;
}> = [
  { id: 'expected_delivery', label: 'Expected delivery', detail: 'Event matches the 4–6 PM care window', Icon: PackageCheck },
  { id: 'repeated_unexpected', label: 'Repeated visitor', detail: 'Two events, no scheduled visitor', Icon: ShieldAlert },
  { id: 'blocked_unlock', label: 'Blocked unlock', detail: 'Beyond the agent’s permissions', Icon: LockKeyhole },
];

const time = (value: string) => new Intl.DateTimeFormat('en-IN', {
  hour: 'numeric', minute: '2-digit', second: '2-digit',
}).format(new Date(value));

export default function HomeView({ onChanged, notify }: { onChanged: () => void; notify: (message: string) => void }) {
  const reduceMotion = useReducedMotion();
  const [home, setHome] = useState<HomeState | null>(null);
  const [workflow, setWorkflow] = useState<HomeWorkflow | null>(null);
  const [running, setRunning] = useState<HomeScenario | null>(null);
  const [accepted, setAccepted] = useState(false);
  const [bedrockStatus, setBedrockStatus] = useState<BedrockStatus | null>(null);
  const [explanation, setExplanation] = useState<CareExplanation | null>(null);
  const [explaining, setExplaining] = useState(false);
  const [checkingBridge, setCheckingBridge] = useState(true);
  const [bridgeError, setBridgeError] = useState<string | null>(null);
  const [pendingOperation, setPendingOperation] = useState<'accept' | 'reset' | 'ring_test' | 'permission' | 'playground' | null>(null);
  const [ringStatus, setRingStatus] = useState<RingStatus | null>(null);
  const [ringDevices, setRingDevices] = useState<RingPlaygroundDevice[]>([]);
  const [ringDeviceId, setRingDeviceId] = useState('');
  const [ringSnapshot, setRingSnapshot] = useState<RingPlaygroundSnapshot | null>(null);
  const [ringError, setRingError] = useState<string | null>(null);
  const actionPending = useRef(false);
  const actionsDisabled = checkingBridge || Boolean(bridgeError) || Boolean(running) || explaining || Boolean(pendingOperation);

  const load = async () => {
    setCheckingBridge(true);
    const [homeResult, bedrockResult, ringResult] = await Promise.allSettled([
      getHome(AbortSignal.timeout(10_000)), getBedrockStatus(AbortSignal.timeout(10_000)), getRingStatus(AbortSignal.timeout(10_000)),
    ]);
    if (ringResult.status === 'fulfilled') setRingStatus(ringResult.value);
    if (bedrockResult.status === 'fulfilled') setBedrockStatus(bedrockResult.value);

    if (homeResult.status === 'fulfilled') {
      setHome(homeResult.value.home);
      setWorkflow(homeResult.value.home.lastWorkflow);
      setBridgeError(null);
    } else {
      setBridgeError('SHARMAN cannot reach the local MCP bridge. No home action can run until the connection is restored.');
    }
    setCheckingBridge(false);
    onChanged();
  };

  useEffect(() => { void load(); }, []);

  const run = async (scenario: HomeScenario) => {
    if (actionsDisabled || actionPending.current) return;
    actionPending.current = true;
    setRunning(scenario);
    setAccepted(false);
    setExplanation(null);
    try {
      const result = await simulateHome(scenario, AbortSignal.timeout(10_000));
      setWorkflow(result);
      await load();
    } catch {
      setBridgeError('The event request could not be confirmed. The last loaded state is preserved. Reconnect to check the recorded result before trying again.');
      onChanged();
      notify('Start the SHARMAN MCP bridge on port 4179 to run SHARMAN safety simulations.');
    } finally {
      actionPending.current = false;
      setRunning(null);
    }
  };

  const accept = async () => {
    if (!workflow || !['expected_delivery', 'repeated_unexpected'].includes(workflow.scenario) || actionsDisabled || actionPending.current) return;
    actionPending.current = true;
    setPendingOperation('accept');
    try {
      const result = await acceptHomeSuggestion(workflow.scenario as 'expected_delivery' | 'repeated_unexpected', AbortSignal.timeout(10_000));
      setAccepted(true);
      notify(result.message);
      onChanged();
    } catch {
      setBridgeError('The approval request could not be confirmed. Reconnect and check the recorded state before trying again.');
      onChanged();
      notify('Approval could not be confirmed. Reconnect to check the result.');
    } finally {
      actionPending.current = false;
      setPendingOperation(null);
    }
  };

  const reset = async () => {
    if (actionsDisabled || actionPending.current) return;
    actionPending.current = true;
    setPendingOperation('reset');
    try {
      const result = await resetHomeDemo(AbortSignal.timeout(10_000));
      setHome(result.home);
      setWorkflow(null);
      setAccepted(false);
      setExplanation(null);
      onChanged();
      notify('SHARMAN Home demo reset.');
    } catch {
      setBridgeError('The reset request could not be confirmed. The last loaded state is preserved. Reconnect to check the recorded result.');
      onChanged();
      notify('The SHARMAN bridge is unavailable.');
    } finally {
      actionPending.current = false;
      setPendingOperation(null);
    }
  };

  const explain = async () => {
    if (actionsDisabled || actionPending.current) return;
    actionPending.current = true;
    setExplaining(true);
    try {
      setExplanation(await explainHomeWithBedrock(AbortSignal.timeout(30_000)));
      onChanged();
    } catch {
      setBridgeError('SHARMAN lost its connection to the local MCP bridge. The deterministic decision remains preserved.');
      onChanged();
      notify('AWS explanation is unavailable. The deterministic SHARMAN decision remains unchanged.');
    } finally {
      actionPending.current = false;
      setExplaining(false);
    }
  };

  const ringAction = async (eventId?: string) => {
    if (actionsDisabled || actionPending.current) return;
    actionPending.current = true;
    setPendingOperation(eventId ? 'permission' : 'ring_test');
    try {
      if (eventId) await checkRingPermission(eventId, AbortSignal.timeout(10_000));
      else await ingestRingTestEvent(AbortSignal.timeout(10_000));
      setAccepted(false);
      setExplanation(null);
      await load();
    } catch {
      setBridgeError('The Ring event result could not be confirmed. Refresh events before retrying.');
    } finally {
      actionPending.current = false;
      setPendingOperation(null);
    }
  };
  const playgroundAction = async (mode: 'devices' | 'inspect' | 'motion' = 'devices') => {
    if (actionsDisabled || actionPending.current) return;
    actionPending.current = true;
    setPendingOperation('playground');
    setRingError(null);
    try {
      if (mode === 'motion') {
        const result = await syncRingPlayground(ringDeviceId, AbortSignal.timeout(25_000));
        notify(result.message ?? (result.status === 'duplicate_ignored' ? 'This Ring event is already recorded. Generate a new Motion event in the Playground.' : 'Ring Developer Playground event ingested.'));
        setAccepted(false); setExplanation(null); await load();
      } else if (mode === 'inspect') {
        setRingSnapshot(await getRingPlaygroundDevice(ringDeviceId, AbortSignal.timeout(35_000)));
      } else {
        const result = await getRingPlaygroundDevices(AbortSignal.timeout(12_000));
        setRingDevices(result.devices);
        const selectedId = result.devices.some((device) => device.id === ringDeviceId) ? ringDeviceId : result.devices.length === 1 ? result.devices[0]!.id : '';
        setRingDeviceId(selectedId);
        setRingSnapshot(null);
        if (selectedId) setRingSnapshot(await getRingPlaygroundDevice(selectedId, AbortSignal.timeout(35_000)));
        notify(result.devices.length ? 'Ring API device discovery succeeded.' : 'Ring API returned no devices.');
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Ring Playground is unavailable. SHARMAN safety simulations remain available.';
      setRingError(message);
      notify(message);
    } finally {
      try { setRingStatus(await getRingStatus(AbortSignal.timeout(5_000))); } catch { /* Keep the provider error separate from Home bridge health. */ }
      actionPending.current = false; setPendingOperation(null);
    }
  };
  const latest = workflow?.event || home?.events[0];
  const latestAgent = workflow?.agent || (latest?.source === 'ring' ? undefined : home?.agents[0]);
  const ringSource = latest?.ring?.ingestionMode === 'ring_developer_playground' ? 'Ring Developer Playground' : latest?.ring?.ingestionMode === 'signed_webhook' ? 'SIGNED RING WEBHOOK' : latest?.source === 'ring' ? 'LOCAL RING-FORMAT TEST' : 'SHARMAN SIMULATOR READY';
  const healthState = checkingBridge ? 'checking' : bridgeError ? 'offline' : home?.status === 'attention' ? 'attention' : 'safe';
  const workflowSteps = workflow ? [
    { label: 'Ring event ingested', detail: workflow.event.source, tone: 'done' },
    { label: 'Household context loaded', detail: 'declared delivery window and visit context', tone: 'done' },
    { label: 'VisitorAgent created', detail: `${workflow.agent.permissions.length} allowed · ${workflow.agent.forbidden.length} forbidden`, tone: 'done' },
    { label: 'Expected visit checked', detail: workflow.event.expectedMatch ? 'expected window matched' : 'no scheduled visit matched', tone: 'done' },
    {
      label: workflow.decision === 'denied' ? `${workflow.agent.denialDetail?.action || 'Capability'} denied` : workflow.decision === 'approval_required' ? 'User approval required' : 'Permission check passed',
      detail: workflow.decision.replace('_', ' '),
      tone: workflow.decision === 'denied' ? 'denied' : workflow.decision === 'approval_required' ? 'attention' : 'done',
    },
    { label: 'Alexa response prepared', detail: 'grounded event facts only', tone: 'done' },
    { label: 'Result recorded', detail: `event ${workflow.event.id.slice(-8)}`, tone: 'done' },
    { label: 'Agent terminated', detail: workflow.agent.terminatedAt === workflow.agent.createdAt ? 'no background authority retained' : 'session closed', tone: 'done' },
  ] : [];

  return <section className="view home-view" aria-busy={checkingBridge || Boolean(running) || explaining || Boolean(pendingOperation)}>
    <div className="home-heading">
      <div>
        <p className="kicker"><House /> SHARMAN HOME</p>
        <h1>A home that understands before it acts.</h1>
        <p>Home activity and safety scenarios, with clear control over every action.</p>
      </div>
      <div className={`home-health ${healthState}`} role="status" aria-live="polite">
        <span /><div><small>HOME STATUS</small><strong>{healthState === 'checking' ? 'Checking bridge…' : healthState === 'offline' ? 'Bridge unavailable' : healthState === 'attention' ? 'Attention needed' : 'Safe & monitored'}</strong></div>
      </div>
    </div>

    {bridgeError && <div className="home-offline" role="alert">
      <AlertTriangle />
      <div><strong>Local bridge unavailable</strong><p>{bridgeError}</p>{home && <span>Showing the last successfully loaded state.</span>}</div>
      <button disabled={checkingBridge} onClick={() => void load()}><RotateCcw />{checkingBridge ? 'Reconnecting…' : 'Retry connection'}</button>
    </div>}

    <div className="home-layout">
      <div className="home-primary">
        <RingPlaygroundPanel status={ringStatus} devices={ringDevices} deviceId={ringDeviceId} snapshot={ringSnapshot} error={ringError} disabled={actionsDisabled} busy={pendingOperation === 'playground'} bridgeAvailable={!bridgeError && !checkingBridge} onSelect={(id) => { setRingDeviceId(id); setRingSnapshot(null); setRingError(null); }} onLoad={() => void playgroundAction()} onRefresh={() => void playgroundAction('inspect')} onMotion={() => void playgroundAction('motion')} />
        <article className="door-card">
          <div className="door-visual">
            <div className="camera-rings"><Video /><i /><i /></div>
            <span><Radio /> {bridgeError ? 'SIMULATOR OFFLINE' : checkingBridge ? 'CHECKING BRIDGE' : 'SHARMAN SAFETY SIMULATIONS'}</span>
          </div>
          <div className="door-copy">
            <div className="door-title"><div><small>SHARMAN SAFETY SIMULATIONS</small><h2>Front door scenarios</h2></div><span className={`online-dot ${bridgeError ? 'offline' : checkingBridge ? 'checking' : ''}`}><i /> {bridgeError ? 'Unavailable' : checkingBridge ? 'Checking…' : 'Ready'}</span></div>
            <div className="context-strip"><PackageCheck /><div><small>EXPECTED CARE EVENT</small><strong>{home?.expectedVisit.label || 'Medicine delivery'}</strong><span>{home?.expectedVisit.window || '4:00–6:00 PM'}</span></div></div>
            <p className="sim-label">Simulated household context · choose a scenario</p>
            <div className="scenario-grid" role="group" aria-label="SHARMAN safety simulation scenarios">
              {scenarios.map(({ id, label, detail, Icon }) => <button key={id} aria-label={`${label}: ${detail}`} disabled={Boolean(running) || actionsDisabled} className={id === 'blocked_unlock' ? 'danger-scenario' : ''} onClick={() => void run(id)}>
                <Icon /><span><strong>{running === id ? 'Processing event…' : label}</strong><small>{detail}</small></span>
              </button>)}
            </div>
            <div className="aws-care-row">
              <button disabled={actionsDisabled} onClick={() => void load()}>Refresh events</button>
              {ringStatus?.localTestEnabled && <button disabled={actionsDisabled} onClick={() => void ringAction()}>{pendingOperation === 'ring_test' ? 'Ingesting…' : 'Send Ring-format test event'}</button>}
            </div>
          </div>
        </article>

        <AnimatePresence mode="wait">
          {latest ? <motion.article key={latest.id} className={`event-card ${latest.severity}`} role="status" aria-live="polite" aria-atomic="true" initial={reduceMotion ? false : { opacity: 0, y: 18 }} animate={{ opacity: 1, y: 0 }} exit={reduceMotion ? undefined : { opacity: 0, y: -10 }}>
            <div className="event-icon">{latest.severity === 'normal' ? <UserRoundCheck /> : <AlertTriangle />}</div>
            <div className="event-main">
              <div className="event-top"><span>{latest.source === 'ring' ? ringSource : 'SHARMAN safety simulation'}</span><time><Clock3 /> {time(latest.at)}</time></div>
              <h2>{latest.title}</h2><p>{latest.summary}</p>
              {latest.ring && <p>{latest.ring.ingestionMode === 'ring_developer_playground' ? latest.ring.delivery === 'user_supplied_capture' ? 'Imported Ring Playground capture.' : 'Ring Developer Playground event history.' : latest.ring.ingestionMode === 'signed_webhook' ? 'Signed Ring notification.' : 'Local test event.'} Event #{latest.ring.sequenceNumber}.</p>}
              {latest.source === 'ring' && latestAgent && workflow?.decision !== 'denied' && <button disabled={actionsDisabled} onClick={() => void ringAction(latest.id)}>Check unlock permission</button>}
              <div className="event-facts">
                <span><Check /> {latest.expectedMatch ? 'Expected window matched' : 'No expected visit matched'}</span>
                <span><Radio /> {latest.occurrenceCount} event{latest.occurrenceCount === 1 ? '' : 's'} observed</span>
              </div>
              {workflow?.agent.denialDetail && <dl className="permission-comparison" aria-label="Permission comparison">
                <div><dt>Requested capability</dt><dd>{workflow.agent.denialDetail.action}</dd></div>
                <div><dt>Required</dt><dd>{workflow.agent.denialDetail.requiredPermission}</dd></div>
                <div><dt>VisitorAgent ceiling</dt><dd>{workflow.agent.denialDetail.delegatedCeiling}</dd></div>
                <div className="denied"><dt>Result</dt><dd>DENIED</dd></div>
              </dl>}
            </div>
            <span className="event-severity">{latest.severity}</span>
          </motion.article> : <div className="home-empty" role="status" aria-live="polite"><Radio /><div><strong>No activity yet</strong><span>Choose a safety scenario to see its workflow.</span></div></div>}
        </AnimatePresence>

        <AnimatePresence>
          {workflow && <motion.article className={`alexa-home-card ${workflow.decision}`} role="status" aria-live="polite" aria-atomic="true" initial={reduceMotion ? false : { opacity: 0, scale: .985 }} animate={{ opacity: 1, scale: 1 }}>
            <div className="alexa-home-orb"><Sparkles /></div>
            <div><small>SIMULATED ALEXA+ EXPERIENCE</small><h3>“{workflow.alexaMessage}”</h3><p>Based on the event and household settings.</p>
              <div className="aws-care-row"><span className={bedrockStatus?.configured ? 'ready' : ''}><Cloud /> {bedrockStatus?.configured ? 'BEDROCK ENABLED' : 'LOCAL SAFE FALLBACK ACTIVE'}</span><button disabled={explaining || actionsDisabled} onClick={() => void explain()}>{explaining ? 'Explaining…' : bedrockStatus?.configured ? 'Explain with Bedrock' : 'View local explanation'}</button></div>
              {explanation && <div className={`aws-explanation ${explanation.mode}`} role="status" aria-live="polite"><strong><Cloud /> {explanation.provider}</strong><p>{explanation.text}</p><span>{explanation.mode === 'bedrock' ? `${explanation.modelId} · ${explanation.region}` : explanation.fallbackReason}</span></div>}
            </div>
            {workflow.suggestedAction && <button disabled={accepted || actionsDisabled} onClick={() => void accept()}>{pendingOperation === 'accept' ? 'Confirming…' : accepted ? <><CheckCircle2 /> Approved</> : workflow.requiresApproval ? <><ShieldAlert /> Review action</> : <><BellRing /> Yes, remind me</>}</button>}
          </motion.article>}
        </AnimatePresence>
      </div>

      <aside className="home-rail">
        <div className="rail-title"><div><small>LIVE WORKFLOW</small><h2>Agent activity</h2></div><button disabled={actionsDisabled} onClick={() => void reset()} title="Reset demo" aria-label={pendingOperation === 'reset' ? 'Resetting SHARMAN Home demo' : 'Reset SHARMAN Home demo'}><RotateCcw /></button></div>
        {latestAgent ? <>
          <div className="agent-identity"><div><Sparkles /></div><span><strong>{latestAgent.name}</strong><small>{latestAgent.status === 'blocked' ? 'Blocked, then destroyed' : 'Completed, then destroyed'}</small></span><em>{latestAgent.status}</em></div>
          <p className="agent-goal">{latestAgent.goal}</p>
          <div className="workflow-line">{workflowSteps.map((step, index) => <div className="workflow-step" key={step.label}><span className={step.tone}>{step.tone === 'denied' ? <X /> : step.tone === 'attention' ? <ShieldAlert /> : <Check />}<b>{step.label}<small>{step.detail}</small></b></span>{index < workflowSteps.length - 1 && <i />}</div>)}</div>
          <div className="permission-panel"><span>MINIMUM PERMISSIONS</span>
            {latestAgent.permissions.map((permission) => <p className="allowed" key={permission}><Check />{permission}</p>)}
            {latestAgent.forbidden.map((permission) => <p className="forbidden" key={permission}><X />{permission}</p>)}
          </div>
        </> : <div className="rail-empty"><Sparkles /><p>A temporary VisitorAgent appears here only while a Ring workflow is evaluated.</p></div>}
        <div className="care-boundary"><LockKeyhole /><p><strong>You stay in control</strong><span>Sensitive actions need explicit permission.</span></p></div>
      </aside>
    </div>
    <div className="simulation-disclosure"><Volume2 /><span><strong>{latest?.source === 'ring' ? 'Ring event provenance' : 'SHARMAN safety simulations'}</strong>{' '}{latest?.source === 'ring' ? 'Event sources are labelled above. Alexa+ speech remains simulated.' : 'The three safety scenarios and Alexa+ speech are simulated.'}</span></div>
  </section>;
}
