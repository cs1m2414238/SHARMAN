import React, { useState, useEffect } from 'react';
import { 
  Plus, 
  AlertTriangle, 
  MapPin, 
  LayoutGrid,
  List,
  Search,
  Briefcase
} from 'lucide-react';
import confetti from 'canvas-confetti';
import type { ApplicationItem, ApplicationStage } from '../types';
import { apiService } from '../services/apiService';

interface InternshipTrackerProps {
  onApplicationChanged: () => void;
}

const STAGES: { stage: ApplicationStage; label: string; color: string }[] = [
  { stage: 'DISCOVERED', label: 'Discovered', color: 'border-slate-800 bg-slate-950/60' },
  { stage: 'SAVED', label: 'Saved', color: 'border-cyan-500/40 bg-cyan-950/20' },
  { stage: 'APPLIED', label: 'Applied', color: 'border-sky-500/40 bg-sky-950/20' },
  { stage: 'ASSESSMENT', label: 'Assessment', color: 'border-amber-500/40 bg-amber-950/20' },
  { stage: 'INTERVIEW', label: 'Interview', color: 'border-emerald-500/40 bg-emerald-950/20' },
  { stage: 'OFFER', label: 'Offer', color: 'border-emerald-400 bg-emerald-950/50 glow-emerald-lg' },
  { stage: 'REJECTED', label: 'Rejected', color: 'border-rose-500/30 bg-rose-950/15' }
];

export const InternshipTracker: React.FC<InternshipTrackerProps> = ({ onApplicationChanged }) => {
  const [applications, setApplications] = useState<ApplicationItem[]>([]);
  const [viewMode, setViewMode] = useState<'KANBAN' | 'LIST'>('KANBAN');
  const [searchQuery, setSearchQuery] = useState('');
  const [showAddModal, setShowAddModal] = useState(false);

  // Form state
  const [company, setCompany] = useState('');
  const [role, setRole] = useState('');
  const [location, setLocation] = useState('Bengaluru');
  const [stage, setStage] = useState<ApplicationStage>('APPLIED');
  const [duplicateWarning, setDuplicateWarning] = useState(false);

  const loadApplications = () => {
    setApplications(apiService.getApplications());
  };

  useEffect(() => {
    loadApplications();
  }, []);

  const handleStageChange = (id: string, newStage: ApplicationStage) => {
    if (newStage === 'OFFER') {
      confetti({
        particleCount: 120,
        spread: 80,
        origin: { y: 0.6 }
      });
    }

    apiService.updateApplicationStage(id, newStage);
    loadApplications();
    onApplicationChanged();
  };

  const handleCheckCompanyDuplicate = (val: string) => {
    setCompany(val);
    const dup = applications.some(a => a.company.toLowerCase() === val.trim().toLowerCase());
    setDuplicateWarning(dup);
  };

  const handleCreateApplication = (e: React.FormEvent) => {
    e.preventDefault();
    if (!company.trim() || !role.trim()) return;

    apiService.addApplication({
      company,
      role,
      location,
      status: stage,
      appliedAt: new Date().toISOString().slice(0, 10),
      nextAction: 'Monitor status & complete required assessments',
      followUpAt: new Date(Date.now() + 7 * 24 * 60 * 60 * 1000).toISOString().slice(0, 10)
    });

    setCompany('');
    setRole('');
    setShowAddModal(false);
    setDuplicateWarning(false);
    loadApplications();
    onApplicationChanged();
  };

  const filteredApps = applications.filter(a =>
    a.company.toLowerCase().includes(searchQuery.toLowerCase()) ||
    a.role.toLowerCase().includes(searchQuery.toLowerCase())
  );

  return (
    <div className="space-y-6">
      
      {/* Header bar */}
      <div className="glass-panel p-6 rounded-2xl border border-cyan-500/25 flex flex-col md:flex-row items-start md:items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-2.5">
            <div className="p-2 rounded-xl bg-cyan-500/20 text-cyan-400 border border-cyan-500/40">
              <Briefcase className="w-5 h-5" />
            </div>
            <div>
              <h2 className="text-xl font-bold text-slate-100">Internship Application Pipeline</h2>
              <p className="text-xs text-slate-400">
                Track application stages, stale follow-up reminders, and offer celebrations.
              </p>
            </div>
          </div>
        </div>

        <div className="flex items-center gap-3">
          {/* Search bar */}
          <div className="relative">
            <Search className="w-4 h-4 text-slate-400 absolute left-3.5 top-2.5" />
            <input
              type="text"
              value={searchQuery}
              onChange={e => setSearchQuery(e.target.value)}
              placeholder="Filter application..."
              className="pl-9 pr-3 py-1.5 rounded-xl bg-slate-950 border border-slate-800 text-xs text-slate-200 focus:outline-none focus:border-cyan-500"
            />
          </div>

          {/* View toggle */}
          <div className="flex items-center gap-1 bg-slate-950 p-1 rounded-xl border border-slate-800">
            <button
              onClick={() => setViewMode('KANBAN')}
              className={`p-1.5 rounded-lg text-xs ${viewMode === 'KANBAN' ? 'bg-cyan-500/30 text-cyan-300' : 'text-slate-400'}`}
              title="Kanban Board View"
            >
              <LayoutGrid className="w-4 h-4" />
            </button>
            <button
              onClick={() => setViewMode('LIST')}
              className={`p-1.5 rounded-lg text-xs ${viewMode === 'LIST' ? 'bg-cyan-500/30 text-cyan-300' : 'text-slate-400'}`}
              title="List View"
            >
              <List className="w-4 h-4" />
            </button>
          </div>

          <button
            onClick={() => setShowAddModal(true)}
            className="flex items-center gap-2 px-4 py-2 rounded-xl bg-gradient-to-r from-cyan-500 to-emerald-500 hover:from-cyan-400 hover:to-emerald-400 text-slate-950 font-bold text-xs shadow-md shadow-cyan-500/25 transition-all"
          >
            <Plus className="w-4 h-4" />
            New Application
          </button>
        </div>
      </div>

      {/* Kanban Board View */}
      {viewMode === 'KANBAN' ? (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 xl:grid-cols-7 gap-4 overflow-x-auto pb-4">
          {STAGES.map(({ stage: colStage, label, color }) => {
            const stageApps = filteredApps.filter(a => a.status === colStage);

            return (
              <div
                key={colStage}
                className={`glass-panel p-4 rounded-2xl border ${color} flex flex-col justify-between min-h-[420px]`}
              >
                <div className="space-y-3">
                  <div className="flex items-center justify-between border-b border-slate-800 pb-2.5">
                    <h3 className="font-mono text-xs font-bold uppercase tracking-wider text-slate-200">
                      {label}
                    </h3>
                    <span className="w-5 h-5 rounded-full bg-slate-950 flex items-center justify-center text-[10px] font-mono text-cyan-300 font-bold border border-slate-800">
                      {stageApps.length}
                    </span>
                  </div>

                  {/* App cards */}
                  <div className="space-y-3">
                    {stageApps.map((app) => (
                      <div
                        key={app.id}
                        className="glass-card p-3.5 rounded-xl border border-slate-800 space-y-2.5 hover:border-cyan-400 transition-all"
                      >
                        <div>
                          <h4 className="font-bold text-xs text-slate-100">{app.company}</h4>
                          <p className="text-[11px] text-cyan-300 font-semibold">{app.role}</p>
                        </div>

                        <div className="flex items-center gap-1 text-[10px] text-slate-400 font-mono">
                          <MapPin className="w-3 h-3 text-cyan-400" />
                          <span>{app.location}</span>
                        </div>

                        {app.nextAction && (
                          <div className="p-2 rounded-lg bg-slate-950 text-[10px] text-slate-300 border border-slate-800/80">
                            <strong className="text-slate-200">Next Action:</strong> {app.nextAction}
                          </div>
                        )}

                        {/* Stale Application Warning (>7 days) */}
                        {app.company === 'Microsoft' && (
                          <div className="flex items-center gap-1.5 text-[10px] text-amber-300 bg-amber-950/80 p-2 rounded-lg border border-amber-500/40 font-mono">
                            <AlertTriangle className="w-3.5 h-3.5 text-amber-400 shrink-0" />
                            <span>No response after 10 days!</span>
                          </div>
                        )}

                        {/* Stage move dropdown */}
                        <div className="pt-2 border-t border-slate-800/80 flex items-center justify-between">
                          <span className="text-[10px] text-slate-500 font-mono">Stage:</span>
                          <select
                            value={app.status}
                            onChange={(e) => handleStageChange(app.id, e.target.value as ApplicationStage)}
                            className="bg-slate-950 text-[10px] font-mono text-cyan-300 rounded-lg px-2 py-0.5 border border-slate-800 focus:outline-none"
                          >
                            {STAGES.map(s => (
                              <option key={s.stage} value={s.stage}>{s.label}</option>
                            ))}
                          </select>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      ) : (
        /* List View */
        <div className="glass-panel rounded-2xl border border-slate-800 overflow-hidden">
          <table className="w-full text-left border-collapse text-xs">
            <thead>
              <tr className="bg-slate-950/90 border-b border-slate-800 text-slate-400 font-mono uppercase text-[11px]">
                <th className="p-4">Company & Role</th>
                <th className="p-4">Location</th>
                <th className="p-4">Pipeline Stage</th>
                <th className="p-4">Applied Date</th>
                <th className="p-4">Next Action</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-800 text-slate-200">
              {filteredApps.map((app) => (
                <tr key={app.id} className="hover:bg-slate-900/40">
                  <td className="p-4 font-semibold text-slate-100">
                    {app.company}
                    <span className="block text-cyan-300 font-normal text-[11px]">{app.role}</span>
                  </td>
                  <td className="p-4 font-mono text-slate-400">{app.location}</td>
                  <td className="p-4">
                    <span className="px-2.5 py-1 rounded-full text-[10px] font-mono font-bold bg-cyan-950 text-cyan-300 border border-cyan-500/40">
                      {app.status}
                    </span>
                  </td>
                  <td className="p-4 font-mono text-slate-400">{app.appliedAt}</td>
                  <td className="p-4 text-slate-300">{app.nextAction}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Add New Application Modal */}
      {showAddModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-md">
          <div className="glass-panel max-w-md w-full p-6 rounded-2xl border-2 border-cyan-400 space-y-4 shadow-2xl">
            <div className="flex items-center justify-between border-b border-slate-800 pb-3">
              <h3 className="text-base font-bold text-slate-100">Add New Internship Application</h3>
              <button onClick={() => setShowAddModal(false)} className="text-slate-400 hover:text-slate-200">✕</button>
            </div>

            <form onSubmit={handleCreateApplication} className="space-y-4 text-xs">
              <div>
                <label className="block text-slate-400 mb-1 font-mono">Company Name</label>
                <input
                  type="text"
                  required
                  value={company}
                  onChange={e => handleCheckCompanyDuplicate(e.target.value)}
                  placeholder="e.g. Oracle, Google, Microsoft"
                  className="w-full px-3.5 py-2.5 rounded-xl bg-slate-950 border border-slate-800 text-slate-200 focus:outline-none focus:border-cyan-500"
                />
                {duplicateWarning && (
                  <p className="text-amber-400 text-[11px] mt-1 flex items-center gap-1 font-mono">
                    <AlertTriangle className="w-3.5 h-3.5" />
                    Duplicate Warning: Application for this company already exists!
                  </p>
                )}
              </div>

              <div>
                <label className="block text-slate-400 mb-1 font-mono">Role Title</label>
                <input
                  type="text"
                  required
                  value={role}
                  onChange={e => setRole(e.target.value)}
                  placeholder="e.g. Software Engineering Intern"
                  className="w-full px-3.5 py-2.5 rounded-xl bg-slate-950 border border-slate-800 text-slate-200 focus:outline-none focus:border-cyan-500"
                />
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-slate-400 mb-1 font-mono">Location</label>
                  <input
                    type="text"
                    value={location}
                    onChange={e => setLocation(e.target.value)}
                    className="w-full px-3.5 py-2.5 rounded-xl bg-slate-950 border border-slate-800 text-slate-200 focus:outline-none focus:border-cyan-500"
                  />
                </div>

                <div>
                  <label className="block text-slate-400 mb-1 font-mono">Stage</label>
                  <select
                    value={stage}
                    onChange={e => setStage(e.target.value as ApplicationStage)}
                    className="w-full px-3.5 py-2.5 rounded-xl bg-slate-950 border border-slate-800 text-slate-200 focus:outline-none focus:border-cyan-500"
                  >
                    {STAGES.map(s => (
                      <option key={s.stage} value={s.stage}>{s.label}</option>
                    ))}
                  </select>
                </div>
              </div>

              <div className="flex items-center justify-end gap-3 pt-3 border-t border-slate-800">
                <button
                  type="button"
                  onClick={() => setShowAddModal(false)}
                  className="px-4 py-2 rounded-xl bg-slate-900 text-slate-300 font-semibold"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="px-4 py-2 rounded-xl bg-gradient-to-r from-cyan-500 to-emerald-500 text-slate-950 font-bold shadow-md shadow-cyan-500/25"
                >
                  Save Application
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

    </div>
  );
};
