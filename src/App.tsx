import React, { useState } from 'react';
import { Navbar } from './components/Navbar';
import { AssistantTerminal } from './components/AssistantTerminal';
import { TaskPlanner } from './components/TaskPlanner';
import { NotificationsCenter } from './components/NotificationsCenter';
import { EmailIntelligence } from './components/EmailIntelligence';
import { InternshipTracker } from './components/InternshipTracker';
import { OpportunityFinder } from './components/OpportunityFinder';
import { CalendarView } from './components/CalendarView';
import { SettingsModal } from './components/SettingsModal';
import { apiService } from './services/apiService';

export const App: React.FC = () => {
  const [activeTab, setActiveTab] = useState<string>('tasks');
  const [isVoiceActive, setIsVoiceActive] = useState<boolean>(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState<boolean>(false);
  const [isOAuthConnected, setIsOAuthConnected] = useState<boolean>(apiService.isOAuthConnected());
  const [refreshKey, setRefreshKey] = useState<number>(0);

  const triggerGlobalRefresh = () => {
    setRefreshKey(prev => prev + 1);
  };

  const notifications = apiService.getNotifications();
  const unreadCount = notifications.filter(n => !n.read).length;

  return (
    <div className="min-h-screen bg-[#040711] text-slate-100 flex flex-col font-sans selection:bg-cyan-500 selection:text-slate-950">
      
      {/* Top Header Navigation */}
      <Navbar
        activeTab={activeTab}
        setActiveTab={setActiveTab}
        isVoiceActive={isVoiceActive}
        setIsVoiceActive={setIsVoiceActive}
        openSettings={() => setIsSettingsOpen(true)}
        isOAuthConnected={isOAuthConnected}
        unreadCount={unreadCount}
      />

      {/* Main Content Area */}
      <main className="flex-1 max-w-7xl w-full mx-auto px-4 lg:px-8 py-6 space-y-6">
        
        {/* Floating / Top Jarvis AI Command Center */}
        <AssistantTerminal
          key={`terminal-${refreshKey}`}
          onTaskCreated={triggerGlobalRefresh}
        />

        {/* Tab Modules */}
        <div key={`tab-content-${refreshKey}`}>
          {activeTab === 'tasks' && (
            <TaskPlanner onTasksChanged={triggerGlobalRefresh} />
          )}

          {activeTab === 'notifications' && (
            <NotificationsCenter onActionTriggered={triggerGlobalRefresh} />
          )}

          {activeTab === 'gmail' && (
            <EmailIntelligence 
              onReminderCreated={triggerGlobalRefresh} 
            />
          )}

          {activeTab === 'tracker' && (
            <InternshipTracker onApplicationChanged={triggerGlobalRefresh} />
          )}

          {activeTab === 'finder' && (
            <OpportunityFinder onSavedChange={triggerGlobalRefresh} />
          )}

          {activeTab === 'calendar' && (
            <CalendarView />
          )}
        </div>

      </main>

      {/* Privacy & Settings Modal */}
      <SettingsModal
        isOpen={isSettingsOpen}
        onClose={() => setIsSettingsOpen(false)}
        isOAuthConnected={isOAuthConnected}
        setIsOAuthConnected={setIsOAuthConnected}
      />

      {/* Footer */}
      <footer className="w-full border-t border-slate-800/80 py-4 text-center text-xs text-slate-500 font-mono">
        <p>SHARMAN — Personal AI Command Center • Java Spring Boot REST Backend & React AI Dashboard</p>
      </footer>

    </div>
  );
};

export default App;
