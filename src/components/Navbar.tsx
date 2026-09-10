import React from 'react';
import { 
  Bot, 
  CheckSquare, 
  Mail, 
  Briefcase, 
  Sparkles, 
  Calendar, 
  ShieldCheck, 
  Settings,
  Mic,
  Bell,
  Cpu
} from 'lucide-react';

interface NavbarProps {
  activeTab: string;
  setActiveTab: (tab: string) => void;
  isVoiceActive: boolean;
  setIsVoiceActive: (active: boolean) => void;
  openSettings: () => void;
  isOAuthConnected: boolean;
  unreadCount?: number;
}

export const Navbar: React.FC<NavbarProps> = ({
  activeTab,
  setActiveTab,
  isVoiceActive,
  setIsVoiceActive,
  openSettings,
  isOAuthConnected,
  unreadCount = 4
}) => {
  const tabs = [
    { id: 'tasks', label: 'Daily Tasks', icon: CheckSquare, count: '4' },
    { id: 'notifications', label: 'Notifications', icon: Bell, count: unreadCount > 0 ? `${unreadCount} NEW` : 'OK' },
    { id: 'gmail', label: 'Gmail Intelligence', icon: Mail, count: '3' },
    { id: 'tracker', label: 'Internship Pipeline', icon: Briefcase, count: '4' },
    { id: 'finder', label: 'Opportunity Finder', icon: Sparkles, count: '89%' },
    { id: 'calendar', label: 'Calendar Sync', icon: Calendar, count: 'OK' },
  ];

  return (
    <header className="sticky top-0 z-40 w-full glass-panel border-b border-cyan-500/25 px-4 lg:px-8 py-3.5 shadow-2xl">
      <div className="max-w-7xl mx-auto flex flex-col md:flex-row items-center justify-between gap-4">
        
        {/* Brand & Arc Reactor AI Logo */}
        <div className="flex items-center gap-3.5">
          <div className="relative flex items-center justify-center w-12 h-12 rounded-2xl bg-slate-950 border-2 border-cyan-400/80 shadow-lg shadow-cyan-500/40 group overflow-hidden">
            <div className="absolute inset-0 border-2 border-dashed border-cyan-400/50 rounded-2xl animate-spin-slow" />
            <Bot className="w-6 h-6 text-cyan-300 stroke-[2.2] relative z-10" />
            <span className="absolute bottom-1 right-1 w-2.5 h-2.5 bg-emerald-400 rounded-full border-2 border-slate-950 animate-ping" />
            <span className="absolute bottom-1 right-1 w-2.5 h-2.5 bg-emerald-400 rounded-full border-2 border-slate-950" />
          </div>

          <div>
            <div className="flex items-center gap-2">
              <h1 className="text-2xl font-black tracking-tight text-gradient-cyan uppercase">
                SHARMAN
              </h1>
              <span className="text-[10px] font-mono font-extrabold tracking-widest px-2.5 py-0.5 rounded-lg bg-cyan-950/90 text-cyan-300 border border-cyan-500/50 shadow-inner uppercase">
                JARVIS OS v1.0
              </span>
            </div>
            <p className="text-xs text-slate-400 font-medium hidden sm:block">
              AI Command Center • Task Automation, Email Intelligence & Internship Pipeline
            </p>
          </div>
        </div>

        {/* Navigation Tabs */}
        <nav className="flex items-center gap-1.5 bg-slate-950/90 p-1.5 rounded-2xl border border-slate-800/90 shadow-inner overflow-x-auto max-w-full">
          {tabs.map((tab) => {
            const Icon = tab.icon;
            const isActive = activeTab === tab.id;
            const isNotify = tab.id === 'notifications';

            return (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`flex items-center gap-2 px-3.5 py-2 rounded-xl text-xs sm:text-sm font-semibold transition-all duration-250 whitespace-nowrap relative ${
                  isActive
                    ? 'bg-gradient-to-r from-cyan-500/30 via-sky-500/20 to-emerald-500/30 text-cyan-200 border border-cyan-400/60 shadow-lg shadow-cyan-500/20'
                    : 'text-slate-400 hover:text-slate-200 hover:bg-slate-900/60'
                }`}
              >
                <Icon className={`w-4 h-4 ${isActive ? 'text-cyan-400 animate-pulse' : isNotify && unreadCount > 0 ? 'text-rose-400 animate-bounce' : 'text-slate-400'}`} />
                <span>{tab.label}</span>
                <span className={`text-[10px] font-mono px-1.5 py-0.2 rounded-full font-bold ${
                  isActive 
                    ? 'bg-cyan-500/40 text-cyan-200 border border-cyan-400/40' 
                    : isNotify && unreadCount > 0
                    ? 'bg-rose-500/30 text-rose-300 border border-rose-500/40'
                    : 'bg-slate-800 text-slate-400'
                }`}>
                  {tab.count}
                </span>

                {isActive && (
                  <span className="absolute bottom-0 left-3 right-3 h-0.5 bg-gradient-to-r from-cyan-400 to-emerald-400 rounded-full" />
                )}
              </button>
            );
          })}
        </nav>

        {/* HUD Live Status Controls */}
        <div className="flex items-center gap-3">
          
          {/* Spring Boot REST API Live Status Pill */}
          <div className="hidden lg:flex items-center gap-2 px-3 py-1.5 rounded-xl bg-slate-950/90 text-slate-300 text-xs font-mono border border-slate-800 shadow-inner">
            <Cpu className="w-3.5 h-3.5 text-cyan-400" />
            <span>Java API :8080</span>
            <span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse" />
          </div>

          {/* Voice Mic Active Trigger */}
          <button
            onClick={() => setIsVoiceActive(!isVoiceActive)}
            title={isVoiceActive ? 'Voice Assistant Listening...' : 'Click to Enable Voice Command Assistant'}
            className={`flex items-center gap-2 px-3.5 py-1.5 rounded-xl text-xs font-mono font-bold transition-all ${
              isVoiceActive
                ? 'bg-cyan-500/30 text-cyan-200 border-2 border-cyan-400 mic-active-pulse'
                : 'bg-slate-950/90 text-slate-400 hover:text-slate-200 border border-slate-800'
            }`}
          >
            <Mic className={`w-4 h-4 ${isVoiceActive ? 'text-cyan-300 animate-bounce' : ''}`} />
            <span className="hidden sm:inline">{isVoiceActive ? 'JARVIS ON' : 'MIC OFF'}</span>
          </button>

          {/* Google OAuth Status */}
          <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-emerald-950/80 text-emerald-300 text-xs font-mono font-semibold border border-emerald-500/40 shadow-sm">
            <ShieldCheck className="w-4 h-4 text-emerald-400" />
            <span className="hidden xl:inline">{isOAuthConnected ? 'OAuth OK' : 'Disconnected'}</span>
          </div>

          {/* Settings Button */}
          <button
            onClick={openSettings}
            className="p-2 rounded-xl bg-slate-950/90 hover:bg-slate-850 text-slate-400 hover:text-slate-200 border border-slate-800 hover:border-cyan-500/50 transition-all"
            title="Settings & Privacy Controls"
          >
            <Settings className="w-4.5 h-4.5" />
          </button>
        </div>

      </div>
    </header>
  );
};
