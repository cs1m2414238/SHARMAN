import type { 
  TaskItem, 
  EmailInsightItem, 
  InternshipOpportunityItem, 
  ApplicationItem, 
  MatchScoreBreakdown,
  NotificationItem
} from '../types';

const STORAGE_KEYS = {
  TASKS: 'sharman_tasks',
  EMAILS: 'sharman_emails',
  OPPORTUNITIES: 'sharman_opportunities',
  APPLICATIONS: 'sharman_applications',
  REMINDERS: 'sharman_reminders',
  NOTIFICATIONS: 'sharman_notifications',
  OAUTH_CONNECTED: 'sharman_oauth_connected'
};

const initialNotifications: NotificationItem[] = [
  {
    id: 'n-1',
    title: '🚨 Overdue Task Alert: Submit Java Assignment',
    message: 'Assignment deadline passed yesterday at 8:00 PM. High priority task remaining in lab queue.',
    type: 'OVERDUE',
    timestamp: '10 minutes ago',
    read: false,
    actionLabel: 'Mark Completed',
    actionType: 'MARK_TASK_DONE'
  },
  {
    id: 'n-2',
    title: '✉️ Gmail Intelligence: Oracle Online Assessment Invitation',
    message: 'Extracted 90-minute HackerRank coding assessment link. Deadline: 11 August at 11:59 PM.',
    type: 'GMAIL_OA',
    timestamp: '1 hour ago',
    read: false,
    actionLabel: 'Schedule Calendar Reminder',
    actionType: 'SCHEDULE_REMINDER'
  },
  {
    id: 'n-3',
    title: '📅 Google STEP Intern Technical Interview Scheduled',
    message: 'Round 1 Technical Interview scheduled for August 9 at 2:00 PM via Google Meet.',
    type: 'GMAIL_INTERVIEW',
    timestamp: '3 hours ago',
    read: false,
    actionLabel: 'View Interview Prep',
    actionType: 'VIEW_INTERVIEW'
  },
  {
    id: 'n-4',
    title: '⚠️ Stale Application Warning: Microsoft SWE Summer Intern',
    message: 'Application has been in "APPLIED" stage for 10 days with no response from recruiter.',
    type: 'STALE_APP',
    timestamp: 'Yesterday',
    read: false,
    actionLabel: 'Send Recruiter Follow-up Email',
    actionType: 'SEND_FOLLOWUP'
  },
  {
    id: 'n-5',
    title: '⚡ Opportunity Deadline Approaching: Atlassian Backend Intern',
    message: 'Application closing date in 4 days. Match score calculated at 82%.',
    type: 'REMINDER',
    timestamp: '2 days ago',
    read: true,
    actionLabel: 'Apply Now',
    actionType: 'APPLY_NOW'
  }
];

const initialTasks: TaskItem[] = [
  {
    id: 't-1',
    title: 'Submit Java Assignment',
    description: 'Complete Object-Oriented Programming & Collections lab assignment',
    priority: 'HIGH',
    status: 'PENDING',
    dueAt: new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString(),
    source: 'VOICE',
    createdAt: new Date().toISOString()
  },
  {
    id: 't-2',
    title: 'Apply to Oracle Software Engineering Internship',
    description: 'Prepare backend resume and apply before 11 August deadline',
    priority: 'HIGH',
    status: 'PENDING',
    dueAt: new Date(Date.now() + 48 * 60 * 60 * 1000).toISOString(),
    source: 'CHAT',
    createdAt: new Date().toISOString()
  },
  {
    id: 't-3',
    title: 'Finish Spring Boot REST API for SHARMAN',
    description: 'Implement JPA repository mapping and Spring Security OAuth2 controllers',
    priority: 'MEDIUM',
    status: 'PENDING',
    dueAt: new Date(Date.now() + 72 * 60 * 60 * 1000).toISOString(),
    source: 'MANUAL',
    createdAt: new Date().toISOString()
  },
  {
    id: 't-4',
    title: 'Check Assessment email from XYZ Corp',
    description: 'Complete HackerRank online code assessment link before link expires',
    priority: 'HIGH',
    status: 'PENDING',
    dueAt: new Date(Date.now() + 12 * 60 * 60 * 1000).toISOString(),
    source: 'GMAIL',
    createdAt: new Date().toISOString()
  }
];

const initialEmails: EmailInsightItem[] = [
  {
    id: 'e-1',
    gmailMessageId: 'msg-101',
    subject: 'Oracle Software Engineering Internship — Online Assessment Link',
    sender: 'careers@oracle.com',
    category: 'ASSESSMENT',
    company: 'Oracle',
    role: 'Software Engineering Intern',
    extractedDeadline: new Date(Date.now() + 3 * 24 * 60 * 60 * 1000).toISOString(),
    requiredAction: 'Complete 90-min Coding Assessment on HackerRank',
    confidenceScore: 0.96,
    processed: false,
    receivedAt: new Date(Date.now() - 4 * 60 * 60 * 1000).toISOString(),
    snippet: 'Dear Applicant, Thank you for applying. Please find your assessment link inside...'
  },
  {
    id: 'e-2',
    gmailMessageId: 'msg-102',
    subject: 'Google STEP Intern 2025 — Technical Interview Invitation',
    sender: 'university-recruiting@google.com',
    category: 'INTERVIEW',
    company: 'Google',
    role: 'STEP Intern 2025',
    extractedDeadline: new Date(Date.now() + 5 * 24 * 60 * 60 * 1000).toISOString(),
    requiredAction: 'Select interview slot on Google Meet portal',
    confidenceScore: 0.98,
    processed: true,
    receivedAt: new Date(Date.now() - 24 * 60 * 60 * 1000).toISOString(),
    snippet: 'We are pleased to invite you to Round 1 Technical Interview for the STEP Internship role...'
  },
  {
    id: 'e-3',
    gmailMessageId: 'msg-103',
    subject: 'Application Confirmation — Microsoft SWE Summer Intern',
    sender: 'jobs@microsoft.com',
    category: 'CONFIRMATION',
    company: 'Microsoft',
    role: 'SWE Summer Intern',
    extractedDeadline: new Date(Date.now() + 14 * 24 * 60 * 60 * 1000).toISOString(),
    requiredAction: 'Awaiting recruiter review',
    confidenceScore: 0.92,
    processed: true,
    receivedAt: new Date(Date.now() - 48 * 60 * 60 * 1000).toISOString(),
    snippet: 'Your application has been received successfully and forwarded to the hiring manager...'
  }
];

const initialOpportunities: InternshipOpportunityItem[] = [
  {
    id: 'o-1',
    company: 'Oracle',
    role: 'Software Engineering Internship',
    location: 'Bengaluru, India',
    skills: ['Java', 'SQL', 'Spring Boot'],
    applicationUrl: 'https://oracle.com/careers',
    deadline: '2026-08-11',
    source: 'Official Career Portal',
    matchScore: 89,
    matchReason: 'Strong backend and Java / Spring Boot match',
    saved: true
  },
  {
    id: 'o-2',
    company: 'Google',
    role: 'STEP Intern 2025',
    location: 'Remote / Bengaluru / Hyderabad',
    skills: ['Python', 'Java', 'Data Structures', 'Algorithms'],
    applicationUrl: 'https://careers.google.com',
    deadline: '2026-08-15',
    source: 'Approved University Portal',
    matchScore: 94,
    matchReason: 'Excellent student eligibility, algorithms skill match & remote flex',
    saved: true
  },
  {
    id: 'o-3',
    company: 'Atlassian',
    role: 'Backend Engineering Intern',
    location: 'Bengaluru (Hybrid)',
    skills: ['Java', 'REST API', 'PostgreSQL'],
    applicationUrl: 'https://atlassian.com/careers',
    deadline: '2026-08-20',
    source: 'Wellfound RSS Feed',
    matchScore: 82,
    matchReason: 'Strong match on REST API & Java backend architecture',
    saved: false
  },
  {
    id: 'o-4',
    company: 'Uber',
    role: 'Backend Systems Intern',
    location: 'Bengaluru',
    skills: ['Java', 'Kafka', 'Microservices'],
    applicationUrl: 'https://uber.com/careers',
    deadline: '2026-08-25',
    source: 'Verified Job Link',
    matchScore: 78,
    matchReason: 'Good match; requires learning basic Kafka concepts',
    saved: false
  }
];

const initialApplications: ApplicationItem[] = [
  {
    id: 'app-1',
    company: 'Oracle',
    role: 'Software Engineering Internship',
    location: 'Bengaluru',
    status: 'ASSESSMENT',
    appliedAt: '2026-08-01',
    nextAction: 'Complete HackerRank OA by 11 August',
    followUpAt: '2026-08-12',
    notes: 'Online assessment link received via Gmail intelligence',
    lastUpdated: new Date().toISOString()
  },
  {
    id: 'app-2',
    company: 'Google',
    role: 'STEP Intern 2025',
    location: 'Remote / Hyderabad',
    status: 'INTERVIEW',
    appliedAt: '2026-07-28',
    nextAction: 'Prepare Graph & Dynamic Programming algorithms',
    followUpAt: '2026-08-09',
    notes: 'Round 1 interview scheduled for 9th August',
    lastUpdated: new Date().toISOString()
  },
  {
    id: 'app-3',
    company: 'Microsoft',
    role: 'SWE Summer Intern',
    location: 'Bengaluru',
    status: 'APPLIED',
    appliedAt: '2026-07-25',
    nextAction: 'Follow up with recruiter if no status change after 7 days',
    followUpAt: '2026-08-04',
    notes: 'No response after 10 days — alert triggered!',
    lastUpdated: new Date().toISOString()
  },
  {
    id: 'app-4',
    company: 'Atlassian',
    role: 'Backend Engineering Intern',
    location: 'Bengaluru',
    status: 'OFFER',
    appliedAt: '2026-07-15',
    nextAction: 'Review offer letter and sign before 15 August',
    followUpAt: '2026-08-10',
    notes: 'Stipend ₹1,20,000/month + housing support',
    lastUpdated: new Date().toISOString()
  }
];

class ApiService {
  private getStorage<T>(key: string, defaultValue: T): T {
    try {
      const data = localStorage.getItem(key);
      return data ? JSON.parse(data) : defaultValue;
    } catch {
      return defaultValue;
    }
  }

  private setStorage<T>(key: string, value: T): void {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch (e) {
      console.error('Storage set error:', e);
    }
  }

  // Notifications API
  getNotifications(): NotificationItem[] {
    return this.getStorage(STORAGE_KEYS.NOTIFICATIONS, initialNotifications);
  }

  markNotificationRead(id: string): void {
    const notifications = this.getNotifications();
    const updated = notifications.map(n => n.id === id ? { ...n, read: true } : n);
    this.setStorage(STORAGE_KEYS.NOTIFICATIONS, updated);
  }

  dismissNotification(id: string): void {
    const notifications = this.getNotifications();
    const updated = notifications.filter(n => n.id !== id);
    this.setStorage(STORAGE_KEYS.NOTIFICATIONS, updated);
  }

  // Tasks API
  getTasks(): TaskItem[] {
    return this.getStorage(STORAGE_KEYS.TASKS, initialTasks);
  }

  saveTask(task: Omit<TaskItem, 'id' | 'createdAt'>): TaskItem {
    const tasks = this.getTasks();
    const newTask: TaskItem = {
      ...task,
      id: 't-' + Date.now(),
      createdAt: new Date().toISOString()
    };
    const updated = [newTask, ...tasks];
    this.setStorage(STORAGE_KEYS.TASKS, updated);
    return newTask;
  }

  updateTaskStatus(id: string, status: TaskItem['status']): void {
    const tasks = this.getTasks();
    const updated = tasks.map(t => t.id === id ? { ...t, status } : t);
    this.setStorage(STORAGE_KEYS.TASKS, updated);
  }

  deleteTask(id: string): void {
    const tasks = this.getTasks();
    const updated = tasks.filter(t => t.id !== id);
    this.setStorage(STORAGE_KEYS.TASKS, updated);
  }

  // Emails API
  getEmails(): EmailInsightItem[] {
    return this.getStorage(STORAGE_KEYS.EMAILS, initialEmails);
  }

  markEmailProcessed(id: string): void {
    const emails = this.getEmails();
    const updated = emails.map(e => e.id === id ? { ...e, processed: true } : e);
    this.setStorage(STORAGE_KEYS.EMAILS, updated);
  }

  // Opportunities API
  getOpportunities(): InternshipOpportunityItem[] {
    return this.getStorage(STORAGE_KEYS.OPPORTUNITIES, initialOpportunities);
  }

  toggleSaveOpportunity(id: string): void {
    const opps = this.getOpportunities();
    const updated = opps.map(o => o.id === id ? { ...o, saved: !o.saved } : o);
    this.setStorage(STORAGE_KEYS.OPPORTUNITIES, updated);
  }

  // Applications API
  getApplications(): ApplicationItem[] {
    return this.getStorage(STORAGE_KEYS.APPLICATIONS, initialApplications);
  }

  updateApplicationStage(id: string, stage: ApplicationItem['status']): void {
    const apps = this.getApplications();
    const updated = apps.map(a => a.id === id ? { ...a, status: stage, lastUpdated: new Date().toISOString() } : a);
    this.setStorage(STORAGE_KEYS.APPLICATIONS, updated);
  }

  addApplication(app: Omit<ApplicationItem, 'id' | 'lastUpdated'>): ApplicationItem {
    const apps = this.getApplications();
    const newApp: ApplicationItem = {
      ...app,
      id: 'app-' + Date.now(),
      lastUpdated: new Date().toISOString()
    };
    const updated = [newApp, ...apps];
    this.setStorage(STORAGE_KEYS.APPLICATIONS, updated);
    return newApp;
  }

  // OAuth status
  isOAuthConnected(): boolean {
    return this.getStorage(STORAGE_KEYS.OAUTH_CONNECTED, true);
  }

  setOAuthConnected(connected: boolean): void {
    this.setStorage(STORAGE_KEYS.OAUTH_CONNECTED, connected);
  }

  // Match score breakdown calculator
  calculateScoreBreakdown(opp: InternshipOpportunityItem): MatchScoreBreakdown {
    const userSkills = ['java', 'sql', 'spring boot', 'react', 'typescript', 'python'];
    const oppSkills = opp.skills.map(s => s.toLowerCase());
    const matched = oppSkills.filter(s => userSkills.includes(s)).length;
    const skillRatio = oppSkills.length > 0 ? matched / oppSkills.length : 0.8;

    const skillMatch = Math.round(skillRatio * 100);
    const studentElig = 100;
    const locationRemote = opp.location.toLowerCase().includes('remote') || opp.location.toLowerCase().includes('bengaluru') ? 95 : 70;
    const deadlineUrgency = 85;
    const companyRelevance = ['oracle', 'google', 'microsoft', 'atlassian', 'uber'].includes(opp.company.toLowerCase()) ? 100 : 75;
    const expRequired = 100;

    const totalScore = Math.round(
      (skillMatch * 0.35) +
      (studentElig * 0.20) +
      (locationRemote * 0.15) +
      (deadlineUrgency * 0.15) +
      (companyRelevance * 0.10) +
      (expRequired * 0.05)
    );

    return {
      skillMatch,
      studentElig,
      locationRemote,
      deadlineUrgency,
      companyRelevance,
      expRequired,
      totalScore
    };
  }
}

export const apiService = new ApiService();
