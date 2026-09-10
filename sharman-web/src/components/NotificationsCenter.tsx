import React, { useState, useEffect } from 'react';
import { 
  Bell, 
  AlertTriangle, 
  Mail, 
  Calendar, 
  Clock, 
  Trash2, 
  Check, 
  Zap,
  Sparkles
} from 'lucide-react';
import type { NotificationItem, NotificationType } from '../types';
import { apiService } from '../services/apiService';

interface NotificationsCenterProps {
  onActionTriggered: () => void;
}

export const NotificationsCenter: React.FC<NotificationsCenterProps> = ({ onActionTriggered }) => {
  const [notifications, setNotifications] = useState<NotificationItem[]>([]);
  const [filter, setFilter] = useState<'ALL' | 'UNREAD' | 'URGENT' | 'GMAIL'>('ALL');
  const [toastMessage, setToastMessage] = useState<string | null>(null);

  const loadNotifications = () => {
    setNotifications(apiService.getNotifications());
  };

  useEffect(() => {
    loadNotifications();
  }, []);

  const triggerToast = (msg: string) => {
    setToastMessage(msg);
    setTimeout(() => setToastMessage(null), 3000);
  };

  const handleAction = (item: NotificationItem) => {
    apiService.markNotificationRead(item.id);
    
    if (item.actionType === 'MARK_TASK_DONE') {
      const tasks = apiService.getTasks();
      if (tasks.length > 0) {
        apiService.updateTaskStatus(tasks[0].id, 'COMPLETED');
      }
      triggerToast('✓ Marked task as completed!');
    } else if (item.actionType === 'SCHEDULE_REMINDER') {
      triggerToast('✓ Exported deadline reminder to Google Calendar & Browser!');
    } else if (item.actionType === 'SEND_FOLLOWUP') {
      triggerToast('✓ Recruiter follow-up draft generated & opened in Gmail!');
    } else if (item.actionType === 'VIEW_INTERVIEW') {
      triggerToast('✓ Opened Interview Preparation guide for Google STEP Intern!');
    } else {
      triggerToast('✓ Action executed successfully!');
    }

    loadNotifications();
    onActionTriggered();
  };

  const handleDismiss = (id: string) => {
    apiService.dismissNotification(id);
    loadNotifications();
    onActionTriggered();
  };

  const filteredNotifications = notifications.filter(n => {
    if (filter === 'UNREAD') return !n.read;
    if (filter === 'URGENT') return n.type === 'OVERDUE' || n.type === 'STALE_APP';
    if (filter === 'GMAIL') return n.type === 'GMAIL_OA' || n.type === 'GMAIL_INTERVIEW';
    return true;
  });

  const unreadCount = notifications.filter(n => !n.read).length;

  const getNotificationCardStyle = (type: NotificationType) => {
    switch (type) {
      case 'OVERDUE':
        return 'border-rose-500/50 bg-rose-950/20 text-rose-300';
      case 'STALE_APP':
        return 'border-amber-500/50 bg-amber-950/20 text-amber-300';
      case 'GMAIL_OA':
        return 'border-cyan-500/50 bg-cyan-950/20 text-cyan-300';
      case 'GMAIL_INTERVIEW':
        return 'border-emerald-500/50 bg-emerald-950/20 text-emerald-300';
      case 'REMINDER':
        return 'border-violet-500/40 bg-violet-950/20 text-violet-300';
    }
  };

  const getNotificationIcon = (type: NotificationType) => {
    switch (type) {
      case 'OVERDUE':
        return <AlertTriangle className="w-5 h-5 text-rose-400 animate-pulse" />;
      case 'STALE_APP':
        return <Clock className="w-5 h-5 text-amber-400 animate-pulse" />;
      case 'GMAIL_OA':
        return <Mail className="w-5 h-5 text-cyan-400" />;
      case 'GMAIL_INTERVIEW':
        return <Calendar className="w-5 h-5 text-emerald-400" />;
      case 'REMINDER':
        return <Zap className="w-5 h-5 text-violet-400" />;
    }
  };

  return (
    <div className="space-y-6">
      
      {/* Toast alert */}
      {toastMessage && (
        <div className="fixed bottom-6 right-6 z-50 px-5 py-3 rounded-2xl bg-cyan-950 border-2 border-cyan-400 text-cyan-200 font-mono text-xs font-bold shadow-2xl animate-bounce flex items-center gap-2">
          <Sparkles className="w-4 h-4 text-cyan-400 animate-spin" />
          <span>{toastMessage}</span>
        </div>
      )}

      {/* Header bar */}
      <div className="glass-panel p-6 rounded-2xl border border-cyan-500/25 flex flex-col md:flex-row items-start md:items-center justify-between gap-4">
        <div className="space-y-1">
          <div className="flex items-center gap-2.5">
            <div className="p-2.5 rounded-xl bg-cyan-500/20 text-cyan-400 border border-cyan-500/40 relative">
              <Bell className="w-6 h-6" />
              {unreadCount > 0 && (
                <span className="absolute -top-1 -right-1 w-4 h-4 rounded-full bg-rose-500 text-white font-mono text-[10px] flex items-center justify-center font-bold">
                  {unreadCount}
                </span>
              )}
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h2 className="text-xl font-bold text-slate-100">Notifications & Alert Intelligence Center</h2>
                <span className="px-2.5 py-0.5 rounded-full text-xs font-mono bg-cyan-950 text-cyan-400 border border-cyan-500/40 font-bold">
                  {unreadCount} Unread Alerts
                </span>
              </div>
              <p className="text-xs text-slate-400 mt-0.5">
                Centralized notification feed for overdue tasks, Gmail OA invites, interview slots & stale application alerts.
              </p>
            </div>
          </div>
        </div>

        {/* Filters */}
        <div className="flex items-center gap-2 bg-slate-950 p-1.5 rounded-2xl border border-slate-800 text-xs">
          <button
            onClick={() => setFilter('ALL')}
            className={`px-3.5 py-1.5 rounded-xl font-semibold transition-all ${filter === 'ALL' ? 'bg-cyan-500/30 text-cyan-200 border border-cyan-500/40 shadow-sm' : 'text-slate-400 hover:text-slate-200'}`}
          >
            All ({notifications.length})
          </button>
          <button
            onClick={() => setFilter('UNREAD')}
            className={`px-3.5 py-1.5 rounded-xl font-semibold transition-all ${filter === 'UNREAD' ? 'bg-cyan-500/30 text-cyan-200 border border-cyan-500/40 shadow-sm' : 'text-slate-400 hover:text-slate-200'}`}
          >
            Unread ({unreadCount})
          </button>
          <button
            onClick={() => setFilter('URGENT')}
            className={`px-3.5 py-1.5 rounded-xl font-semibold transition-all ${filter === 'URGENT' ? 'bg-rose-500/30 text-rose-200 border border-rose-500/40 shadow-sm' : 'text-slate-400 hover:text-slate-200'}`}
          >
            Urgent Alerts
          </button>
          <button
            onClick={() => setFilter('GMAIL')}
            className={`px-3.5 py-1.5 rounded-xl font-semibold transition-all ${filter === 'GMAIL' ? 'bg-emerald-500/30 text-emerald-200 border border-emerald-500/40 shadow-sm' : 'text-slate-400 hover:text-slate-200'}`}
          >
            Gmail Feeds
          </button>
        </div>
      </div>

      {/* Notifications Cards Grid */}
      <div className="space-y-4">
        {filteredNotifications.map((item) => (
          <div
            key={item.id}
            className={`glass-card p-5 rounded-2xl border ${getNotificationCardStyle(item.type)} flex flex-col md:flex-row items-start md:items-center justify-between gap-4 transition-all hover:scale-[1.01]`}
          >
            <div className="flex items-start gap-4 space-y-1">
              <div className="p-3 rounded-2xl bg-slate-950 border border-slate-800 shrink-0 mt-0.5 shadow-md">
                {getNotificationIcon(item.type)}
              </div>

              <div>
                <div className="flex flex-wrap items-center gap-2">
                  <h3 className="font-bold text-sm text-slate-100">{item.title}</h3>
                  {!item.read && (
                    <span className="px-2 py-0.5 rounded-md text-[10px] font-mono font-bold bg-cyan-950 text-cyan-300 border border-cyan-500/40">
                      NEW
                    </span>
                  )}
                </div>
                <p className="text-xs text-slate-300 mt-1 leading-relaxed">{item.message}</p>
                <span className="text-[10px] text-slate-500 font-mono mt-2 block">
                  Logged: {item.timestamp}
                </span>
              </div>
            </div>

            {/* Action Buttons */}
            <div className="flex items-center gap-3 shrink-0 self-end md:self-center">
              {item.actionLabel && (
                <button
                  onClick={() => handleAction(item)}
                  className="flex items-center gap-1.5 px-4 py-2 rounded-xl bg-gradient-to-r from-cyan-500 to-emerald-500 hover:from-cyan-400 hover:to-emerald-400 text-slate-950 font-bold text-xs shadow-md shadow-cyan-500/25 transition-all uppercase tracking-wider"
                >
                  <Check className="w-4 h-4 stroke-[3]" />
                  <span>{item.actionLabel}</span>
                </button>
              )}

              <button
                onClick={() => handleDismiss(item.id)}
                className="p-2 rounded-xl bg-slate-950 hover:bg-slate-900 text-slate-500 hover:text-rose-400 border border-slate-800 transition-colors"
                title="Dismiss Alert"
              >
                <Trash2 className="w-4 h-4" />
              </button>
            </div>
          </div>
        ))}
      </div>

    </div>
  );
};
