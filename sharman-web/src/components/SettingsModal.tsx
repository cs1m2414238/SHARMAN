import React from 'react';
import { ShieldCheck, Trash2, CheckCircle2, X } from 'lucide-react';
import { apiService } from '../services/apiService';

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  isOAuthConnected: boolean;
  setIsOAuthConnected: (connected: boolean) => void;
}

export const SettingsModal: React.FC<SettingsModalProps> = ({
  isOpen,
  onClose,
  isOAuthConnected,
  setIsOAuthConnected
}) => {
  if (!isOpen) return null;

  const handleDisconnectOAuth = () => {
    apiService.setOAuthConnected(false);
    setIsOAuthConnected(false);
  };

  const handleReconnectOAuth = () => {
    apiService.setOAuthConnected(true);
    setIsOAuthConnected(true);
  };

  const handleClearData = () => {
    if (confirm('Are you sure you want to clear local cache and reset SHARMAN state?')) {
      localStorage.clear();
      window.location.reload();
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-sm">
      <div className="glass-panel max-w-lg w-full p-6 rounded-2xl border border-cyan-500/40 space-y-5 shadow-2xl">
        
        {/* Header */}
        <div className="flex items-center justify-between border-b border-slate-800 pb-3">
          <div className="flex items-center gap-2">
            <ShieldCheck className="w-5 h-5 text-cyan-400" />
            <h3 className="text-base font-bold text-slate-100">Privacy, Security & OAuth 2.0 Settings</h3>
          </div>
          <button onClick={onClose} className="text-slate-400 hover:text-slate-200">
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Security Overview */}
        <div className="space-y-3 text-xs">
          
          {/* OAuth Status Card */}
          <div className="p-4 rounded-xl bg-slate-900 border border-slate-800 space-y-2">
            <div className="flex items-center justify-between">
              <span className="font-semibold text-slate-200">Google OAuth 2.0 Authentication</span>
              <span className={`px-2 py-0.5 rounded text-[10px] font-mono font-bold ${
                isOAuthConnected ? 'bg-emerald-950 text-emerald-400 border border-emerald-500/30' : 'bg-rose-950 text-rose-400 border border-rose-500/30'
              }`}>
                {isOAuthConnected ? 'CONNECTED' : 'DISCONNECTED'}
              </span>
            </div>

            <p className="text-slate-400 text-[11px]">
              SHARMAN uses standard OAuth PKCE flow. We NEVER store or ask for your Google passwords.
            </p>

            <div className="pt-2">
              {isOAuthConnected ? (
                <button
                  onClick={handleDisconnectOAuth}
                  className="px-3 py-1.5 rounded-lg bg-rose-950/80 text-rose-300 border border-rose-500/30 hover:bg-rose-900 text-xs font-semibold"
                >
                  Disconnect Gmail OAuth
                </button>
              ) : (
                <button
                  onClick={handleReconnectOAuth}
                  className="px-3 py-1.5 rounded-lg bg-emerald-600 text-slate-950 font-bold text-xs shadow-md shadow-emerald-600/30"
                >
                  Connect with Google OAuth
                </button>
              )}
            </div>
          </div>

          {/* Privacy Checklist */}
          <div className="p-4 rounded-xl bg-slate-900/60 border border-slate-800 space-y-2">
            <h4 className="font-mono font-bold text-[11px] text-cyan-400 uppercase">Privacy & Data Audit</h4>
            
            <ul className="space-y-1.5 text-slate-300 text-[11px]">
              <li className="flex items-center gap-1.5">
                <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" />
                <span><strong>Minimum Permissions:</strong> Only <code>gmail.readonly</code> & <code>calendar.events</code> scopes requested.</span>
              </li>
              <li className="flex items-center gap-1.5">
                <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" />
                <span><strong>Token Encryption:</strong> OAuth tokens encrypted at rest via AES-256 GCM.</span>
              </li>
              <li className="flex items-center gap-1.5">
                <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" />
                <span><strong>Action Confirmation:</strong> Confirms before scheduling calendar events or creating tasks.</span>
              </li>
            </ul>
          </div>

          {/* Reset / Data Wipe */}
          <div className="p-4 rounded-xl bg-rose-950/20 border border-rose-500/30 flex items-center justify-between gap-3">
            <div>
              <h4 className="font-semibold text-rose-300">Wipe Stored Application Data</h4>
              <p className="text-[10px] text-slate-400">Permanently delete stored tasks, email insights & applications.</p>
            </div>
            <button
              onClick={handleClearData}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-rose-600 hover:bg-rose-500 text-white font-semibold text-xs shadow-sm"
            >
              <Trash2 className="w-3.5 h-3.5" />
              Reset Cache
            </button>
          </div>

        </div>

        {/* Modal Close Footer */}
        <div className="pt-3 border-t border-slate-800 text-right">
          <button
            onClick={onClose}
            className="px-4 py-2 rounded-xl bg-cyan-600 text-white font-semibold text-xs"
          >
            Close Settings
          </button>
        </div>

      </div>
    </div>
  );
};
