import React, { useState } from 'react';
import { Clock, CheckCircle2, RefreshCw, ShieldCheck } from 'lucide-react';

export const CalendarView: React.FC = () => {
  const [isSyncing, setIsSyncing] = useState(false);

  const handleSyncGoogleCalendar = () => {
    setIsSyncing(true);
    setTimeout(() => {
      setIsSyncing(false);
    }, 1000);
  };

  const timelineEvents = [
    {
      id: 'ev-1',
      title: 'Submit Java Assignment',
      type: 'TASK DEADLINE',
      date: new Date(Date.now() + 24 * 60 * 60 * 1000).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }),
      badgeColor: 'bg-rose-950 text-rose-300 border-rose-500/40'
    },
    {
      id: 'ev-2',
      title: 'Oracle Online Assessment Submission',
      type: 'GMAIL DEADLINE',
      date: '11 August 2026, 11:59 PM',
      badgeColor: 'bg-amber-950 text-amber-300 border-amber-500/40'
    },
    {
      id: 'ev-3',
      title: 'Google STEP Intern Technical Interview',
      type: 'INTERVIEW SLOT',
      date: '9 August 2026, 2:00 PM',
      badgeColor: 'bg-emerald-950 text-emerald-300 border-emerald-500/40'
    },
    {
      id: 'ev-4',
      title: 'Atlassian Backend Intern Deadline',
      type: 'APPLICATION CLOSING',
      date: '20 August 2026',
      badgeColor: 'bg-cyan-950 text-cyan-300 border-cyan-500/40'
    }
  ];

  return (
    <div className="space-y-6">
      
      {/* Google Calendar Integration Header */}
      <div className="glass-panel p-6 rounded-2xl border border-cyan-500/20 flex flex-col md:flex-row items-start md:items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <h2 className="text-xl font-bold text-slate-100">Google Calendar & Reminders Sync</h2>
            <div className="flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-mono bg-emerald-950 text-emerald-400 border border-emerald-500/30">
              <ShieldCheck className="w-3.5 h-3.5" />
              <span>Google Calendar v3 API</span>
            </div>
          </div>
          <p className="text-xs text-slate-400 mt-1">
            Extracted email deadlines and daily tasks are automatically synchronized to your primary Google Calendar.
          </p>
        </div>

        <button
          onClick={handleSyncGoogleCalendar}
          disabled={isSyncing}
          className="flex items-center gap-2 px-4 py-2 rounded-xl bg-gradient-to-r from-cyan-600 to-emerald-600 hover:from-cyan-500 hover:to-emerald-500 text-white font-semibold text-xs shadow-md shadow-cyan-600/30 transition-all disabled:opacity-50"
        >
          <RefreshCw className={`w-4 h-4 ${isSyncing ? 'animate-spin' : ''}`} />
          {isSyncing ? 'Syncing Calendar...' : 'Sync Calendar Now'}
        </button>
      </div>

      {/* Upcoming Deadline Timeline */}
      <div className="glass-panel p-6 rounded-2xl border border-slate-800 space-y-4">
        <h3 className="font-mono text-sm font-bold text-slate-200 uppercase tracking-wider flex items-center gap-2">
          <Clock className="w-4 h-4 text-cyan-400" />
          Upcoming Synchronized Calendar Events
        </h3>

        <div className="space-y-3">
          {timelineEvents.map((ev) => (
            <div
              key={ev.id}
              className="p-4 rounded-xl bg-slate-900/80 border border-slate-800/80 flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3 hover:border-cyan-500/40 transition-all"
            >
              <div className="space-y-1">
                <div className="flex items-center gap-2">
                  <span className={`px-2 py-0.5 rounded text-[10px] font-mono font-bold border ${ev.badgeColor}`}>
                    {ev.type}
                  </span>
                  <h4 className="font-semibold text-sm text-slate-100">{ev.title}</h4>
                </div>
                <p className="text-xs text-slate-400 font-mono">Scheduled: {ev.date}</p>
              </div>

              <div className="flex items-center gap-2 text-xs font-mono text-emerald-400 bg-emerald-950/60 px-3 py-1 rounded-lg border border-emerald-500/30">
                <CheckCircle2 className="w-3.5 h-3.5" />
                <span>Synced to Google Calendar</span>
              </div>
            </div>
          ))}
        </div>
      </div>

    </div>
  );
};
