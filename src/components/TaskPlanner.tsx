import React, { useState, useEffect } from 'react';
import { 
  Plus, 
  Clock, 
  CheckCircle2, 
  Trash2, 
  Sun, 
  Moon,
  CheckSquare
} from 'lucide-react';
import type { TaskItem, PriorityLevel, TaskStatus } from '../types';
import { apiService } from '../services/apiService';

interface TaskPlannerProps {
  onTasksChanged: () => void;
}

export const TaskPlanner: React.FC<TaskPlannerProps> = ({ onTasksChanged }) => {
  const [tasks, setTasks] = useState<TaskItem[]>([]);
  const [filter, setFilter] = useState<'ALL' | 'PENDING' | 'OVERDUE' | 'COMPLETED'>('ALL');
  const [showAddModal, setShowAddModal] = useState<boolean>(false);
  const [showPlanModal, setShowPlanModal] = useState<boolean>(false);
  const [planType, setPlanType] = useState<'MORNING' | 'EVENING'>('MORNING');

  // Form states
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [priority, setPriority] = useState<PriorityLevel>('HIGH');
  const [dueDate, setDueDate] = useState(new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString().slice(0, 16));

  const loadTasks = () => {
    setTasks(apiService.getTasks());
  };

  useEffect(() => {
    loadTasks();
  }, []);

  const handleCreateTask = (e: React.FormEvent) => {
    e.preventDefault();
    if (!title.trim()) return;

    apiService.saveTask({
      title,
      description,
      priority,
      status: 'PENDING',
      dueAt: new Date(dueDate).toISOString(),
      source: 'MANUAL'
    });

    setTitle('');
    setDescription('');
    setShowAddModal(false);
    loadTasks();
    onTasksChanged();
  };

  const handleToggleTaskStatus = (task: TaskItem) => {
    const nextStatus: TaskStatus = task.status === 'COMPLETED' ? 'PENDING' : 'COMPLETED';
    apiService.updateTaskStatus(task.id, nextStatus);
    loadTasks();
    onTasksChanged();
  };

  const handleDeleteTask = (id: string) => {
    apiService.deleteTask(id);
    loadTasks();
    onTasksChanged();
  };

  const filteredTasks = tasks.filter(t => {
    if (filter === 'PENDING') return t.status === 'PENDING';
    if (filter === 'OVERDUE') return t.status === 'OVERDUE';
    if (filter === 'COMPLETED') return t.status === 'COMPLETED';
    return true;
  });

  const pendingCount = tasks.filter(t => t.status === 'PENDING').length;
  const completedCount = tasks.filter(t => t.status === 'COMPLETED').length;
  const overdueCount = tasks.filter(t => t.status === 'OVERDUE').length;
  const completionPercentage = tasks.length > 0 ? Math.round((completedCount / tasks.length) * 100) : 0;

  return (
    <div className="space-y-6">
      
      {/* Top Banner & Stat Cards */}
      <div className="glass-panel p-6 rounded-2xl border border-cyan-500/25 space-y-5">
        <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-4">
          <div className="flex items-center gap-3">
            <div className="p-2.5 rounded-xl bg-cyan-500/20 text-cyan-400 border border-cyan-500/40">
              <CheckSquare className="w-6 h-6" />
            </div>
            <div>
              <h2 className="text-xl font-bold text-slate-100">Daily Task Command & Planning</h2>
              <p className="text-xs text-slate-400 mt-0.5">
                AI prioritizes your workload by strict deadlines, internship applications, and course assignments.
              </p>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-3">
            <button
              onClick={() => { setPlanType('MORNING'); setShowPlanModal(true); }}
              className="flex items-center gap-2 px-3.5 py-2 rounded-xl bg-amber-500/15 text-amber-300 border border-amber-500/40 hover:bg-amber-500/25 text-xs font-semibold transition-all shadow-sm"
            >
              <Sun className="w-4 h-4 text-amber-400" />
              Morning Plan
            </button>

            <button
              onClick={() => { setPlanType('EVENING'); setShowPlanModal(true); }}
              className="flex items-center gap-2 px-3.5 py-2 rounded-xl bg-indigo-500/15 text-indigo-300 border border-indigo-500/40 hover:bg-indigo-500/25 text-xs font-semibold transition-all shadow-sm"
            >
              <Moon className="w-4 h-4 text-indigo-400" />
              Evening Summary
            </button>

            <button
              onClick={() => setShowAddModal(true)}
              className="flex items-center gap-2 px-4 py-2 rounded-xl bg-gradient-to-r from-cyan-500 to-emerald-500 hover:from-cyan-400 hover:to-emerald-400 text-slate-950 font-bold text-xs shadow-md shadow-cyan-500/25 transition-all"
            >
              <Plus className="w-4 h-4" />
              Add Task
            </button>
          </div>
        </div>

        {/* Progress Bar & Stats */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 pt-2">
          <div className="p-3.5 rounded-xl bg-slate-950/80 border border-slate-800/80 space-y-1">
            <span className="text-[11px] text-slate-400 font-mono">Completion Rate</span>
            <div className="flex items-center justify-between">
              <span className="text-lg font-black text-cyan-300 font-mono">{completionPercentage}%</span>
              <div className="w-12 bg-slate-900 h-2 rounded-full overflow-hidden border border-slate-800">
                <div className="bg-cyan-400 h-full" style={{ width: `${completionPercentage}%` }} />
              </div>
            </div>
          </div>

          <div className="p-3.5 rounded-xl bg-slate-950/80 border border-slate-800/80 space-y-1">
            <span className="text-[11px] text-slate-400 font-mono">Pending Tasks</span>
            <span className="text-lg font-black text-slate-100 font-mono block">{pendingCount}</span>
          </div>

          <div className="p-3.5 rounded-xl bg-slate-950/80 border border-slate-800/80 space-y-1">
            <span className="text-[11px] text-rose-400 font-mono">Overdue</span>
            <span className="text-lg font-black text-rose-400 font-mono block">{overdueCount}</span>
          </div>

          <div className="p-3.5 rounded-xl bg-slate-950/80 border border-slate-800/80 space-y-1">
            <span className="text-[11px] text-emerald-400 font-mono">Completed</span>
            <span className="text-lg font-black text-emerald-400 font-mono block">{completedCount}</span>
          </div>
        </div>

      </div>

      {/* Filter Tabs */}
      <div className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-2 bg-slate-950/90 p-1.5 rounded-2xl border border-slate-800 text-xs shadow-inner">
          <button
            onClick={() => setFilter('ALL')}
            className={`px-3.5 py-1.5 rounded-xl font-semibold transition-all ${filter === 'ALL' ? 'bg-cyan-500/25 text-cyan-300 border border-cyan-500/40 shadow-sm' : 'text-slate-400 hover:text-slate-200'}`}
          >
            All Tasks ({tasks.length})
          </button>
          <button
            onClick={() => setFilter('PENDING')}
            className={`px-3.5 py-1.5 rounded-xl font-semibold transition-all ${filter === 'PENDING' ? 'bg-cyan-500/25 text-cyan-300 border border-cyan-500/40 shadow-sm' : 'text-slate-400 hover:text-slate-200'}`}
          >
            Pending ({pendingCount})
          </button>
          <button
            onClick={() => setFilter('OVERDUE')}
            className={`px-3.5 py-1.5 rounded-xl font-semibold transition-all ${filter === 'OVERDUE' ? 'bg-rose-500/25 text-rose-300 border border-rose-500/40 shadow-sm' : 'text-slate-400 hover:text-slate-200'}`}
          >
            Overdue ({overdueCount})
          </button>
          <button
            onClick={() => setFilter('COMPLETED')}
            className={`px-3.5 py-1.5 rounded-xl font-semibold transition-all ${filter === 'COMPLETED' ? 'bg-emerald-500/25 text-emerald-300 border border-emerald-500/40 shadow-sm' : 'text-slate-400 hover:text-slate-200'}`}
          >
            Completed ({completedCount})
          </button>
        </div>
      </div>

      {/* Task Cards Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {filteredTasks.map((task) => {
          const isDone = task.status === 'COMPLETED';
          const isHigh = task.priority === 'HIGH';

          return (
            <div
              key={task.id}
              className={`glass-card p-5 rounded-2xl flex flex-col justify-between space-y-4 border ${
                isDone
                  ? 'border-emerald-500/30 opacity-75 bg-slate-950/40'
                  : isHigh
                  ? 'border-cyan-500/40'
                  : 'border-slate-800'
              }`}
            >
              <div className="space-y-2.5">
                <div className="flex items-start justify-between gap-3">
                  <div className="flex items-start gap-3">
                    <button
                      onClick={() => handleToggleTaskStatus(task)}
                      className={`mt-0.5 w-5 h-5 rounded-md border flex items-center justify-center transition-all ${
                        isDone
                          ? 'bg-emerald-500 border-emerald-400 text-slate-950 font-bold'
                          : 'border-slate-600 hover:border-cyan-400 text-transparent'
                      }`}
                    >
                      <CheckCircle2 className="w-4 h-4" />
                    </button>
                    <div>
                      <h3 className={`font-semibold text-sm ${isDone ? 'line-through text-slate-400' : 'text-slate-100'}`}>
                        {task.title}
                      </h3>
                      {task.description && (
                        <p className="text-xs text-slate-400 mt-1 leading-relaxed">{task.description}</p>
                      )}
                    </div>
                  </div>

                  {/* Priority Tag */}
                  <span
                    className={`px-2.5 py-0.5 rounded-full text-[10px] font-mono uppercase font-bold tracking-wider ${
                      task.priority === 'HIGH'
                        ? 'bg-rose-950/90 text-rose-300 border border-rose-500/40'
                        : task.priority === 'MEDIUM'
                        ? 'bg-amber-950/90 text-amber-300 border border-amber-500/40'
                        : 'bg-slate-900 text-slate-400 border border-slate-800'
                    }`}
                  >
                    {task.priority}
                  </span>
                </div>
              </div>

              {/* Task Meta Footer */}
              <div className="flex items-center justify-between text-xs text-slate-400 pt-3 border-t border-slate-800/80 font-mono">
                <div className="flex items-center gap-1.5">
                  <Clock className="w-3.5 h-3.5 text-cyan-400" />
                  <span>{new Date(task.dueAt).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })}</span>
                </div>

                <div className="flex items-center gap-2">
                  <span className="text-[10px] px-2 py-0.5 rounded-md bg-slate-950 text-cyan-400 border border-slate-800">
                    {task.source}
                  </span>

                  <button
                    onClick={() => handleDeleteTask(task.id)}
                    className="p-1 rounded text-slate-500 hover:text-rose-400 transition-colors"
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                </div>
              </div>
            </div>
          );
        })}
      </div>

      {/* Add Task Modal */}
      {showAddModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-md">
          <div className="glass-panel max-w-md w-full p-6 rounded-2xl border-2 border-cyan-400 shadow-2xl space-y-4">
            <div className="flex items-center justify-between border-b border-slate-800 pb-3">
              <h3 className="text-base font-bold text-slate-100">Add New Task</h3>
              <button onClick={() => setShowAddModal(false)} className="text-slate-400 hover:text-slate-200">✕</button>
            </div>

            <form onSubmit={handleCreateTask} className="space-y-4 text-xs">
              <div>
                <label className="block text-slate-400 mb-1 font-mono">Task Title</label>
                <input
                  type="text"
                  required
                  value={title}
                  onChange={e => setTitle(e.target.value)}
                  placeholder="e.g. Complete Java lab assignment"
                  className="w-full px-3.5 py-2.5 rounded-xl bg-slate-950 border border-slate-800 text-slate-200 focus:outline-none focus:border-cyan-500"
                />
              </div>

              <div>
                <label className="block text-slate-400 mb-1 font-mono">Description (Optional)</label>
                <textarea
                  rows={2}
                  value={description}
                  onChange={e => setDescription(e.target.value)}
                  placeholder="Additional details..."
                  className="w-full px-3.5 py-2.5 rounded-xl bg-slate-950 border border-slate-800 text-slate-200 focus:outline-none focus:border-cyan-500"
                />
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-slate-400 mb-1 font-mono">Priority</label>
                  <select
                    value={priority}
                    onChange={e => setPriority(e.target.value as PriorityLevel)}
                    className="w-full px-3.5 py-2.5 rounded-xl bg-slate-950 border border-slate-800 text-slate-200 focus:outline-none focus:border-cyan-500"
                  >
                    <option value="HIGH">HIGH</option>
                    <option value="MEDIUM">MEDIUM</option>
                    <option value="LOW">LOW</option>
                  </select>
                </div>

                <div>
                  <label className="block text-slate-400 mb-1 font-mono">Due Date & Time</label>
                  <input
                    type="datetime-local"
                    value={dueDate}
                    onChange={e => setDueDate(e.target.value)}
                    className="w-full px-3 py-2 rounded-xl bg-slate-950 border border-slate-800 text-slate-200 focus:outline-none focus:border-cyan-500"
                  />
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
                  className="px-4 py-2 rounded-xl bg-gradient-to-r from-cyan-500 to-emerald-500 text-slate-950 font-bold text-xs shadow-md shadow-cyan-500/25"
                >
                  Save Task
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Morning / Evening Plan Modal */}
      {showPlanModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-md">
          <div className="glass-panel max-w-lg w-full p-6 rounded-2xl border-2 border-cyan-400 shadow-2xl space-y-4">
            <div className="flex items-center justify-between border-b border-slate-800 pb-3">
              <div className="flex items-center gap-2">
                {planType === 'MORNING' ? <Sun className="w-5 h-5 text-amber-400" /> : <Moon className="w-5 h-5 text-indigo-400" />}
                <h3 className="text-base font-bold text-slate-100">
                  {planType === 'MORNING' ? 'SHARMAN Morning Briefing' : 'SHARMAN Evening Progress Report'}
                </h3>
              </div>
              <button onClick={() => setShowPlanModal(false)} className="text-slate-400 hover:text-slate-200">✕</button>
            </div>

            <div className="space-y-3 text-xs leading-relaxed text-slate-200 font-sans">
              {planType === 'MORNING' ? (
                <>
                  <p className="text-amber-300 font-semibold">☀️ Good morning! Here is your AI-optimized priority schedule for today:</p>
                  <ul className="list-disc list-inside space-y-2 pl-2 text-slate-300">
                    <li><strong className="text-cyan-300">1. Submit Java Assignment</strong> — High Priority (Due tomorrow at 8 PM).</li>
                    <li><strong className="text-cyan-300">2. Apply to Oracle Internship</strong> — Application closes in 2 days (Skill match 89%).</li>
                    <li><strong className="text-cyan-300">3. Finish Spring Boot REST API</strong> — Mandatory backend requirement.</li>
                    <li><strong className="text-cyan-300">4. Review HackerRank Assessment</strong> — Link extracted from Gmail intelligence.</li>
                  </ul>
                </>
              ) : (
                <>
                  <p className="text-indigo-300 font-semibold">🌙 Evening Progress Summary:</p>
                  <div className="p-3.5 rounded-xl bg-slate-950 border border-slate-800 space-y-1 font-mono">
                    <p>• Completed: <span className="text-emerald-400 font-bold">{completedCount} tasks</span></p>
                    <p>• Pending: <span className="text-cyan-400 font-bold">{pendingCount} tasks</span> remaining for tomorrow</p>
                    <p>• Overdue: <span className="text-rose-400 font-bold">{overdueCount} items</span></p>
                  </div>
                  <p className="text-slate-400">Great job today! Rest well to stay sharp for tomorrow's technical interview preparation.</p>
                </>
              )}
            </div>

            <div className="pt-3 border-t border-slate-800 text-right">
              <button
                onClick={() => setShowPlanModal(false)}
                className="px-4 py-2 rounded-xl bg-cyan-600 hover:bg-cyan-500 text-white font-semibold text-xs"
              >
                Close Summary
              </button>
            </div>
          </div>
        </div>
      )}

    </div>
  );
};
