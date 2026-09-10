import React, { useState, useEffect } from 'react';
import { 
  Sparkles, 
  CheckCircle2, 
  Clock, 
  Building2, 
  FileCheck, 
  RefreshCw, 
  Check,
  MailCheck,
  Zap
} from 'lucide-react';
import type { EmailInsightItem } from '../types';
import { apiService } from '../services/apiService';

interface EmailIntelligenceProps {
  onReminderCreated: () => void;
}

export const EmailIntelligence: React.FC<EmailIntelligenceProps> = ({
  onReminderCreated
}) => {
  const [emails, setEmails] = useState<EmailInsightItem[]>([]);
  const [selectedEmail, setSelectedEmail] = useState<EmailInsightItem | null>(null);
  const [isScanning, setIsScanning] = useState<boolean>(false);
  const [showApprovalModal, setShowApprovalModal] = useState<boolean>(false);

  const loadEmails = () => {
    setEmails(apiService.getEmails());
  };

  useEffect(() => {
    loadEmails();
  }, []);

  const handleScanGmail = () => {
    setIsScanning(true);
    setTimeout(() => {
      setIsScanning(false);
      loadEmails();
    }, 1200);
  };

  const handleApproveReminder = (email: EmailInsightItem) => {
    setSelectedEmail(email);
    setShowApprovalModal(true);
  };

  const confirmCreateReminder = () => {
    if (!selectedEmail) return;

    apiService.saveTask({
      title: `[Gmail Action] ${selectedEmail.company} — ${selectedEmail.requiredAction}`,
      description: `Deadline: ${new Date(selectedEmail.extractedDeadline).toLocaleDateString()}. Extracted from Gmail message ID: ${selectedEmail.gmailMessageId}`,
      priority: 'HIGH',
      status: 'PENDING',
      dueAt: selectedEmail.extractedDeadline,
      source: 'GMAIL'
    });

    apiService.markEmailProcessed(selectedEmail.id);
    setShowApprovalModal(false);
    setSelectedEmail(null);
    loadEmails();
    onReminderCreated();
  };

  const getCategoryBadge = (category: EmailInsightItem['category']) => {
    switch (category) {
      case 'ASSESSMENT':
        return 'bg-amber-950/90 text-amber-300 border-amber-500/50 shadow-sm shadow-amber-500/20';
      case 'INTERVIEW':
        return 'bg-emerald-950/90 text-emerald-300 border-emerald-500/50 shadow-sm shadow-emerald-500/20';
      case 'CONFIRMATION':
        return 'bg-cyan-950/90 text-cyan-300 border-cyan-500/50 shadow-sm shadow-cyan-500/20';
      case 'REJECTION':
        return 'bg-rose-950/90 text-rose-300 border-rose-500/50 shadow-sm shadow-rose-500/20';
      case 'DEADLINE':
        return 'bg-violet-950/90 text-violet-300 border-violet-500/50 shadow-sm shadow-violet-500/20';
    }
  };

  return (
    <div className="space-y-6">
      
      {/* OAuth Control Header */}
      <div className="glass-panel p-6 rounded-2xl border border-cyan-500/25 flex flex-col md:flex-row items-start md:items-center justify-between gap-4">
        <div className="space-y-1">
          <div className="flex items-center gap-2.5">
            <div className="p-2 rounded-xl bg-cyan-500/20 text-cyan-400 border border-cyan-500/40">
              <MailCheck className="w-5 h-5" />
            </div>
            <div>
              <h2 className="text-xl font-bold text-slate-100">Gmail Intelligence & Deadline Extractor</h2>
              <p className="text-xs text-slate-400">
                Event-driven email monitoring scans mailbox changes for OA links, interview slots, and approaching deadlines.
              </p>
            </div>
          </div>
        </div>

        <button
          onClick={handleScanGmail}
          disabled={isScanning}
          className="flex items-center gap-2 px-4 py-2.5 rounded-xl bg-gradient-to-r from-cyan-500 to-emerald-500 hover:from-cyan-400 hover:to-emerald-400 text-slate-950 font-bold text-xs shadow-lg shadow-cyan-500/25 transition-all disabled:opacity-50"
        >
          <RefreshCw className={`w-4 h-4 ${isScanning ? 'animate-spin' : ''}`} />
          {isScanning ? 'Scanning Mailbox...' : 'Scan Gmail Now'}
        </button>
      </div>

      {/* Extracted Email Insight Cards */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-5">
        {emails.map((email) => (
          <div
            key={email.id}
            className={`glass-card p-6 rounded-2xl flex flex-col justify-between space-y-4 border ${
              email.processed ? 'border-slate-800/80 opacity-75' : 'border-cyan-500/40 glow-cyan-lg'
            }`}
          >
            <div className="space-y-3.5">
              
              {/* Category & Confidence Badge */}
              <div className="flex items-center justify-between gap-2">
                <span className={`px-3 py-1 rounded-full text-[10px] font-mono font-extrabold tracking-wider uppercase border ${getCategoryBadge(email.category)}`}>
                  {email.category}
                </span>

                <div className="flex items-center gap-1 text-[11px] font-mono text-cyan-300 bg-cyan-950/80 px-2.5 py-0.5 rounded-lg border border-cyan-500/40">
                  <Zap className="w-3 h-3 text-cyan-400" />
                  <span>{Math.round(email.confidenceScore * 100)}% AI Accuracy</span>
                </div>
              </div>

              {/* Subject & Company */}
              <div>
                <h3 className="font-bold text-sm text-slate-100 line-clamp-2 leading-snug">
                  {email.subject}
                </h3>
                <div className="flex items-center gap-2 text-xs text-slate-400 mt-1.5 font-sans">
                  <Building2 className="w-3.5 h-3.5 text-cyan-400 shrink-0" />
                  <span className="font-semibold text-cyan-300">{email.company}</span> • <span>{email.role}</span>
                </div>
              </div>

              {/* Required Action & Deadline Box */}
              <div className="p-3.5 rounded-xl bg-slate-950/90 border border-slate-800/90 space-y-2 text-xs">
                <div className="flex items-start gap-2">
                  <FileCheck className="w-4 h-4 text-emerald-400 shrink-0 mt-0.5" />
                  <p className="text-slate-200 font-medium">
                    <strong className="text-slate-100">Action:</strong> {email.requiredAction}
                  </p>
                </div>

                <div className="flex items-center gap-2 text-amber-300 font-mono text-[11px] pt-1">
                  <Clock className="w-3.5 h-3.5 text-amber-400" />
                  <span>Deadline: {new Date(email.extractedDeadline).toLocaleDateString()}</span>
                </div>
              </div>
            </div>

            {/* Bottom Approval Footer */}
            <div className="pt-3 border-t border-slate-800/80 flex items-center justify-between gap-2">
              <span className="text-[10px] text-slate-500 font-mono">
                MSG: {email.gmailMessageId}
              </span>

              {email.processed ? (
                <span className="flex items-center gap-1.5 text-xs text-emerald-400 font-bold font-mono">
                  <CheckCircle2 className="w-4 h-4" />
                  REMINDER ACTIVE
                </span>
              ) : (
                <button
                  onClick={() => handleApproveReminder(email)}
                  className="flex items-center gap-1.5 px-3.5 py-1.5 rounded-xl bg-gradient-to-r from-cyan-500 to-emerald-500 hover:from-cyan-400 hover:to-emerald-400 text-slate-950 font-bold text-xs shadow-md shadow-cyan-500/20 transition-all uppercase tracking-wider"
                >
                  <Sparkles className="w-3.5 h-3.5" />
                  Approve Reminder
                </button>
              )}
            </div>
          </div>
        ))}
      </div>

      {/* Confirmation Dialog */}
      {showApprovalModal && selectedEmail && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-md">
          <div className="glass-panel max-w-md w-full p-6 rounded-2xl border-2 border-cyan-400 shadow-2xl space-y-4">
            <div className="flex items-center justify-between border-b border-slate-800 pb-3">
              <div className="flex items-center gap-2">
                <Sparkles className="w-5 h-5 text-cyan-400 animate-spin" />
                <h3 className="text-base font-bold text-slate-100">Confirm Calendar & Browser Reminder</h3>
              </div>
              <button onClick={() => setShowApprovalModal(false)} className="text-slate-400 hover:text-slate-200">✕</button>
            </div>

            <div className="space-y-2.5 text-xs text-slate-200 font-sans">
              <p><strong className="text-cyan-300 font-mono">Company:</strong> {selectedEmail.company}</p>
              <p><strong className="text-cyan-300 font-mono">Role:</strong> {selectedEmail.role}</p>
              <p><strong className="text-cyan-300 font-mono">Required Action:</strong> {selectedEmail.requiredAction}</p>
              <p><strong className="text-cyan-300 font-mono">Extracted Deadline:</strong> {new Date(selectedEmail.extractedDeadline).toLocaleString()}</p>
              
              <div className="p-3 rounded-xl bg-cyan-950/60 border border-cyan-500/40 text-[11px] text-cyan-300 font-mono mt-2">
                SHARMAN will create a task with browser notifications & export to Google Calendar.
              </div>
            </div>

            <div className="flex items-center justify-end gap-3 pt-3 border-t border-slate-800">
              <button
                onClick={() => setShowApprovalModal(false)}
                className="px-4 py-2 rounded-xl bg-slate-900 text-slate-300 font-semibold text-xs"
              >
                Cancel
              </button>
              <button
                onClick={confirmCreateReminder}
                className="flex items-center gap-2 px-4 py-2 rounded-xl bg-gradient-to-r from-cyan-500 to-emerald-500 text-slate-950 font-extrabold text-xs shadow-lg shadow-cyan-500/30"
              >
                <Check className="w-4 h-4 stroke-[3]" />
                Approve & Schedule
              </button>
            </div>
          </div>
        </div>
      )}

    </div>
  );
};
