import React, { useState, useEffect } from 'react';
import { 
  Search, 
  MapPin, 
  Calendar, 
  ExternalLink, 
  Bookmark, 
  Info,
  TrendingUp,
  X,
  Sparkles,
  Building2
} from 'lucide-react';
import type { InternshipOpportunityItem, MatchScoreBreakdown } from '../types';
import { apiService } from '../services/apiService';

interface OpportunityFinderProps {
  onSavedChange: () => void;
}

export const OpportunityFinder: React.FC<OpportunityFinderProps> = ({ onSavedChange }) => {
  const [opportunities, setOpportunities] = useState<InternshipOpportunityItem[]>([]);
  const [selectedOpp, setSelectedOpp] = useState<InternshipOpportunityItem | null>(null);
  const [scoreBreakdown, setScoreBreakdown] = useState<MatchScoreBreakdown | null>(null);
  const [searchQuery, setSearchQuery] = useState('');

  const loadOpps = () => {
    setOpportunities(apiService.getOpportunities());
  };

  useEffect(() => {
    loadOpps();
  }, []);

  const handleToggleSave = (id: string) => {
    apiService.toggleSaveOpportunity(id);
    loadOpps();
    onSavedChange();
  };

  const handleShowBreakdown = (opp: InternshipOpportunityItem) => {
    setSelectedOpp(opp);
    setScoreBreakdown(apiService.calculateScoreBreakdown(opp));
  };

  const filteredOpps = opportunities.filter(o => 
    o.company.toLowerCase().includes(searchQuery.toLowerCase()) ||
    o.role.toLowerCase().includes(searchQuery.toLowerCase()) ||
    o.skills.some(s => s.toLowerCase().includes(searchQuery.toLowerCase()))
  );

  return (
    <div className="space-y-6">
      
      {/* Header Banner */}
      <div className="glass-panel p-6 rounded-2xl border border-cyan-500/25 flex flex-col md:flex-row items-start md:items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <div className="p-2 rounded-xl bg-cyan-500/20 text-cyan-400 border border-cyan-500/40">
              <Sparkles className="w-5 h-5 animate-pulse" />
            </div>
            <div>
              <h2 className="text-xl font-bold text-slate-100">Opportunity Finder & AI Match Engine</h2>
              <p className="text-xs text-slate-400 mt-0.5">
                Evaluates open roles against your skills, student eligibility, location, and deadline urgency.
              </p>
            </div>
          </div>
        </div>

        {/* Search Input */}
        <div className="relative w-full md:w-80">
          <Search className="w-4 h-4 text-slate-400 absolute left-3.5 top-3" />
          <input
            type="text"
            value={searchQuery}
            onChange={e => setSearchQuery(e.target.value)}
            placeholder="Filter by skill (e.g. Java, Python), company..."
            className="w-full pl-10 pr-4 py-2 rounded-xl bg-slate-950/90 border border-slate-800 text-xs text-slate-200 focus:outline-none focus:border-cyan-500/60 shadow-inner"
          />
        </div>
      </div>

      {/* Opportunity Cards Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
        {filteredOpps.map((opp) => (
          <div
            key={opp.id}
            className="glass-card p-6 rounded-2xl border border-cyan-500/30 flex flex-col justify-between space-y-4 hover:border-cyan-400 transition-all"
          >
            <div className="space-y-3.5">
              
              {/* Header: Match Ring Badge & Info/Bookmark */}
              <div className="flex items-center justify-between gap-3">
                <div className="flex items-center gap-2">
                  <div className="flex items-center gap-2 px-3 py-1.5 rounded-xl bg-gradient-to-r from-cyan-950 to-slate-900 border border-cyan-500/50 text-cyan-300 text-xs font-mono font-bold shadow-md shadow-cyan-500/10">
                    <TrendingUp className="w-4 h-4 text-emerald-400 animate-pulse" />
                    <span>Match: {opp.matchScore}%</span>
                  </div>

                  <button
                    onClick={() => handleShowBreakdown(opp)}
                    className="flex items-center gap-1 px-2.5 py-1 rounded-lg bg-slate-900/90 hover:bg-slate-800 text-slate-400 hover:text-cyan-300 text-xs font-mono border border-slate-800 transition-colors"
                  >
                    <Info className="w-3.5 h-3.5 text-cyan-400" />
                    <span>Why Matched?</span>
                  </button>
                </div>

                <button
                  onClick={() => handleToggleSave(opp.id)}
                  className={`p-2 rounded-xl border transition-all ${
                    opp.saved
                      ? 'bg-amber-500/20 text-amber-400 border-amber-500/40 shadow-sm'
                      : 'bg-slate-900 text-slate-400 border-slate-800 hover:text-slate-200'
                  }`}
                  title={opp.saved ? 'Saved in Applications' : 'Save Opportunity'}
                >
                  <Bookmark className="w-4 h-4" />
                </button>
              </div>

              {/* Title & Role */}
              <div>
                <div className="flex items-center gap-2">
                  <Building2 className="w-4 h-4 text-cyan-400 shrink-0" />
                  <h3 className="font-bold text-base text-slate-100">{opp.company}</h3>
                </div>
                <p className="text-sm font-semibold text-cyan-300 mt-0.5">{opp.role}</p>
                
                <div className="flex flex-wrap items-center gap-3 text-xs text-slate-400 mt-2 font-mono">
                  <span className="flex items-center gap-1">
                    <MapPin className="w-3.5 h-3.5 text-cyan-400" />
                    {opp.location}
                  </span>
                  •
                  <span className="flex items-center gap-1 text-amber-300">
                    <Calendar className="w-3.5 h-3.5 text-amber-400" />
                    Deadline: {new Date(opp.deadline).toLocaleDateString([], { day: 'numeric', month: 'short' })}
                  </span>
                </div>
              </div>

              {/* Required Skills Chips */}
              <div className="flex flex-wrap items-center gap-1.5 pt-1">
                {opp.skills.map((skill, idx) => (
                  <span
                    key={idx}
                    className="px-2.5 py-1 rounded-lg bg-slate-950 text-cyan-300 border border-slate-800 text-xs font-mono font-medium"
                  >
                    {skill}
                  </span>
                ))}
              </div>

              {/* Match Reason Textbox */}
              <div className="p-3 rounded-xl bg-slate-950/90 border border-slate-800/90 text-xs text-slate-300">
                <strong className="text-cyan-400 font-mono">AI Assessment:</strong> {opp.matchReason}
              </div>

            </div>

            {/* Bottom Actions */}
            <div className="pt-3 border-t border-slate-800/80 flex items-center justify-between gap-3">
              <span className="text-[11px] text-slate-400 font-mono">
                Source: {opp.source}
              </span>

              <a
                href={opp.applicationUrl}
                target="_blank"
                rel="noreferrer"
                className="flex items-center gap-1.5 px-4 py-2 rounded-xl bg-gradient-to-r from-cyan-500 to-emerald-500 hover:from-cyan-400 hover:to-emerald-400 text-slate-950 font-bold text-xs shadow-md shadow-cyan-500/25 transition-all"
              >
                <span>Apply Today</span>
                <ExternalLink className="w-3.5 h-3.5" />
              </a>
            </div>
          </div>
        ))}
      </div>

      {/* 6-Factor Score Breakdown Modal */}
      {selectedOpp && scoreBreakdown && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-md">
          <div className="glass-panel max-w-md w-full p-6 rounded-2xl border-2 border-cyan-400 shadow-2xl space-y-4">
            <div className="flex items-center justify-between border-b border-slate-800 pb-3">
              <div>
                <h3 className="text-base font-bold text-slate-100">{selectedOpp.company} — Match Breakdown</h3>
                <p className="text-xs text-slate-400">Total Algorithmic Score: <span className="text-cyan-400 font-mono font-bold text-sm">{scoreBreakdown.totalScore}%</span></p>
              </div>
              <button onClick={() => setSelectedOpp(null)} className="text-slate-400 hover:text-slate-200">
                <X className="w-5 h-5" />
              </button>
            </div>

            <div className="space-y-3 text-xs">
              <div className="space-y-1">
                <div className="flex justify-between font-mono text-[11px]">
                  <span className="text-slate-300">Skill Match (35% weight)</span>
                  <span className="text-cyan-400 font-bold">{scoreBreakdown.skillMatch}%</span>
                </div>
                <div className="w-full bg-slate-950 h-2.5 rounded-full overflow-hidden border border-slate-800">
                  <div className="bg-gradient-to-r from-cyan-500 to-sky-400 h-full rounded-full" style={{ width: `${scoreBreakdown.skillMatch}%` }} />
                </div>
              </div>

              <div className="space-y-1">
                <div className="flex justify-between font-mono text-[11px]">
                  <span className="text-slate-300">Student Eligibility (20% weight)</span>
                  <span className="text-emerald-400 font-bold">{scoreBreakdown.studentElig}%</span>
                </div>
                <div className="w-full bg-slate-950 h-2.5 rounded-full overflow-hidden border border-slate-800">
                  <div className="bg-gradient-to-r from-emerald-500 to-teal-400 h-full rounded-full" style={{ width: `${scoreBreakdown.studentElig}%` }} />
                </div>
              </div>

              <div className="space-y-1">
                <div className="flex justify-between font-mono text-[11px]">
                  <span className="text-slate-300">Location / Remote Flex (15% weight)</span>
                  <span className="text-sky-400 font-bold">{scoreBreakdown.locationRemote}%</span>
                </div>
                <div className="w-full bg-slate-950 h-2.5 rounded-full overflow-hidden border border-slate-800">
                  <div className="bg-sky-400 h-full rounded-full" style={{ width: `${scoreBreakdown.locationRemote}%` }} />
                </div>
              </div>

              <div className="space-y-1">
                <div className="flex justify-between font-mono text-[11px]">
                  <span className="text-slate-300">Application Deadline Urgency (15% weight)</span>
                  <span className="text-amber-400 font-bold">{scoreBreakdown.deadlineUrgency}%</span>
                </div>
                <div className="w-full bg-slate-950 h-2.5 rounded-full overflow-hidden border border-slate-800">
                  <div className="bg-amber-400 h-full rounded-full" style={{ width: `${scoreBreakdown.deadlineUrgency}%` }} />
                </div>
              </div>

              <div className="space-y-1">
                <div className="flex justify-between font-mono text-[11px]">
                  <span className="text-slate-300">Company Relevance (10% weight)</span>
                  <span className="text-indigo-400 font-bold">{scoreBreakdown.companyRelevance}%</span>
                </div>
                <div className="w-full bg-slate-950 h-2.5 rounded-full overflow-hidden border border-slate-800">
                  <div className="bg-indigo-400 h-full rounded-full" style={{ width: `${scoreBreakdown.companyRelevance}%` }} />
                </div>
              </div>

              <div className="space-y-1">
                <div className="flex justify-between font-mono text-[11px]">
                  <span className="text-slate-300">Experience Required (5% weight)</span>
                  <span className="text-violet-400 font-bold">{scoreBreakdown.expRequired}%</span>
                </div>
                <div className="w-full bg-slate-950 h-2.5 rounded-full overflow-hidden border border-slate-800">
                  <div className="bg-violet-400 h-full rounded-full" style={{ width: `${scoreBreakdown.expRequired}%` }} />
                </div>
              </div>
            </div>

            <div className="pt-3 border-t border-slate-800 text-right">
              <button
                onClick={() => setSelectedOpp(null)}
                className="px-4 py-2 rounded-xl bg-cyan-600 hover:bg-cyan-500 text-white font-semibold text-xs"
              >
                Close Breakdown
              </button>
            </div>
          </div>
        </div>
      )}

    </div>
  );
};
