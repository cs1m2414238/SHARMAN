export type PriorityLevel = 'HIGH' | 'MEDIUM' | 'LOW';
export type TaskStatus = 'PENDING' | 'COMPLETED' | 'OVERDUE';

export interface TaskItem {
  id: string;
  title: string;
  description?: string;
  priority: PriorityLevel;
  status: TaskStatus;
  dueAt: string;
  source: 'VOICE' | 'CHAT' | 'GMAIL' | 'MANUAL';
  createdAt: string;
}

export interface ReminderItem {
  id: string;
  taskId: string;
  remindAt: string;
  channel: 'BROWSER' | 'EMAIL' | 'TELEGRAM';
  sent: boolean;
  message: string;
}

export type EmailCategory = 'ASSESSMENT' | 'INTERVIEW' | 'CONFIRMATION' | 'REJECTION' | 'DEADLINE';

export interface EmailInsightItem {
  id: string;
  gmailMessageId: string;
  subject: string;
  sender: string;
  category: EmailCategory;
  company: string;
  role: string;
  extractedDeadline: string;
  requiredAction: string;
  confidenceScore: number;
  processed: boolean;
  receivedAt: string;
  snippet: string;
}

export interface InternshipOpportunityItem {
  id: string;
  company: string;
  role: string;
  location: string;
  skills: string[];
  applicationUrl: string;
  deadline: string;
  source: string;
  matchScore: number;
  matchReason: string;
  saved: boolean;
}

export type ApplicationStage = 'DISCOVERED' | 'SAVED' | 'APPLIED' | 'ASSESSMENT' | 'INTERVIEW' | 'OFFER' | 'REJECTED';

export interface ApplicationItem {
  id: string;
  opportunityId?: string;
  company: string;
  role: string;
  location: string;
  status: ApplicationStage;
  appliedAt: string;
  nextAction: string;
  followUpAt?: string;
  notes?: string;
  lastUpdated: string;
}

export interface ToolCallRequest {
  id: string;
  toolName: string;
  parameters: Record<string, any>;
  description: string;
  status: 'PENDING' | 'APPROVED' | 'REJECTED' | 'EXECUTED';
}

export interface ChatMessage {
  id: string;
  sender: 'USER' | 'SHARMAN';
  text: string;
  timestamp: string;
  toolCall?: ToolCallRequest;
}

export interface MatchScoreBreakdown {
  skillMatch: number;
  studentElig: number;
  locationRemote: number;
  deadlineUrgency: number;
  companyRelevance: number;
  expRequired: number;
  totalScore: number;
}

export type NotificationType = 'OVERDUE' | 'GMAIL_OA' | 'GMAIL_INTERVIEW' | 'STALE_APP' | 'REMINDER';

export interface NotificationItem {
  id: string;
  title: string;
  message: string;
  type: NotificationType;
  timestamp: string;
  read: boolean;
  actionLabel?: string;
  actionType?: string;
}
