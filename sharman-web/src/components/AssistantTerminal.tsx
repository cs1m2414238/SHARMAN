import React, { useState, useEffect, useRef } from 'react';
import { 
  Bot, 
  Send, 
  Mic, 
  MicOff, 
  Volume2, 
  VolumeX, 
  Sparkles, 
  Check, 
  X, 
  Terminal,
  ChevronDown,
  ChevronUp
} from 'lucide-react';
import type { ChatMessage, ToolCallRequest } from '../types';
import { apiService } from '../services/apiService';

interface AssistantTerminalProps {
  onTaskCreated: () => void;
}

export const AssistantTerminal: React.FC<AssistantTerminalProps> = ({
  onTaskCreated
}) => {
  const [isOpen, setIsOpen] = useState<boolean>(true);
  const [inputText, setInputText] = useState<string>('');
  const [isListening, setIsListening] = useState<boolean>(false);
  const [ttsEnabled, setTtsEnabled] = useState<boolean>(true);
  const [pendingToolCall, setPendingToolCall] = useState<ToolCallRequest | null>(null);

  const [messages, setMessages] = useState<ChatMessage[]>([
    {
      id: 'm-1',
      sender: 'SHARMAN',
      text: 'Good day! I am SHARMAN, your Jarvis-style AI Command Assistant. I continuously organize your tasks, Gmail internship emails, application deadlines, and calendar schedules.',
      timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
    }
  ]);

  const chatEndRef = useRef<HTMLDivElement>(null);
  const recognitionRef = useRef<any>(null);

  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages, pendingToolCall]);

  useEffect(() => {
    if (typeof window !== 'undefined') {
      const SpeechRecognition = (window as any).SpeechRecognition || (window as any).webkitSpeechRecognition;
      if (SpeechRecognition) {
        const recognition = new SpeechRecognition();
        recognition.continuous = false;
        recognition.interimResults = false;
        recognition.lang = 'en-US';

        recognition.onresult = (event: any) => {
          const transcript = event.results[0][0].transcript;
          setInputText(transcript);
          setIsListening(false);
          handleSendMessage(transcript);
        };

        recognition.onerror = () => { setIsListening(false); };
        recognition.onend = () => { setIsListening(false); };
        recognitionRef.current = recognition;
      }
    }
  }, []);

  const toggleListening = () => {
    if (isListening) {
      recognitionRef.current?.stop();
      setIsListening(false);
    } else {
      try {
        recognitionRef.current?.start();
        setIsListening(true);
      } catch (err) {
        console.error('Speech recognition error:', err);
      }
    }
  };

  const speakText = (text: string) => {
    if (!ttsEnabled || typeof window === 'undefined' || !('speechSynthesis' in window)) return;
    window.speechSynthesis.cancel();
    const utterance = new SpeechSynthesisUtterance(text);
    utterance.rate = 1.0;
    utterance.pitch = 1.0;
    window.speechSynthesis.speak(utterance);
  };

  const handleSendMessage = (textToSend?: string) => {
    const query = textToSend || inputText;
    if (!query.trim()) return;

    const userMsg: ChatMessage = {
      id: 'm-' + Date.now(),
      sender: 'USER',
      text: query,
      timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
    };

    setMessages(prev => [...prev, userMsg]);
    if (!textToSend) setInputText('');

    processAiIntent(query);
  };

  const processAiIntent = (query: string) => {
    const qLower = query.toLowerCase();

    // Task creation intent
    if (qLower.includes('remind me') || qLower.includes('add task') || qLower.includes('submit assignment')) {
      const toolCall: ToolCallRequest = {
        id: 'tc-' + Date.now(),
        toolName: 'createTask',
        parameters: {
          title: query.replace(/(remind me to|add task to|add task)/gi, '').trim() || 'Submit Assignment',
          dueAt: 'Tomorrow at 8:00 PM',
          priority: 'HIGH'
        },
        description: `Create Task & Schedule Reminders for: "${query.replace(/(remind me to|add task to)/gi, '').trim() || 'Submit Assignment'}"`,
        status: 'PENDING'
      };

      setPendingToolCall(toolCall);
      const botResponse = `I parsed your command into a backend tool call. Confirm action execution below:`;
      addBotMessage(botResponse);
      speakText('I have created a task tool call. Please confirm execution.');
      return;
    }

    // Today's summary
    if (qLower.includes('today') || qLower.includes('do today') || qLower.includes('briefing')) {
      const briefText = `Here is your day summary:\n1. Complete Java assignment — due tomorrow.\n2. Apply to Oracle internship — deadline in 2 days.\n3. Finish Spring Boot REST API.\n4. Check assessment email from XYZ.`;
      addBotMessage(briefText);
      speakText('Here is your daily task summary: Complete Java assignment, apply to Oracle internship, and finish your Spring Boot API.');
      return;
    }

    // Oracle Deadline Reminder
    if (qLower.includes('oracle') || qLower.includes('deadline')) {
      const toolCall: ToolCallRequest = {
        id: 'tc-' + Date.now(),
        toolName: 'createCalendarReminders',
        parameters: {
          company: 'Oracle',
          deadline: '11 August at 11:59 PM',
          reminderDates: ['8 August', '10 August', '11 August at 6 PM']
        },
        description: `Set 3 Google Calendar & Browser Reminders for Oracle Internship Application Deadline (11 August).`,
        status: 'PENDING'
      };

      setPendingToolCall(toolCall);
      const botResponse = `Oracle application closes 11 August at 11:59 PM. I created 3 reminder slots (8 Aug, 10 Aug, 11 Aug 6 PM). Confirm tool execution?`;
      addBotMessage(botResponse);
      speakText('The Oracle application closes on August 11th. Would you like me to schedule reminders for August 8th, 10th, and 11th?');
      return;
    }

    const fallbackText = `I have received: "${query}". Monitoring tasks, Gmail emails, and application deadlines. Ask "What should I do today?" or "Remind me to..."`;
    addBotMessage(fallbackText);
    speakText(fallbackText);
  };

  const addBotMessage = (text: string) => {
    setTimeout(() => {
      setMessages(prev => [
        ...prev,
        {
          id: 'm-' + Date.now(),
          sender: 'SHARMAN',
          text,
          timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
        }
      ]);
    }, 400);
  };

  const handleApproveTool = () => {
    if (!pendingToolCall) return;

    if (pendingToolCall.toolName === 'createTask') {
      apiService.saveTask({
        title: pendingToolCall.parameters.title,
        description: 'Auto-generated via SHARMAN voice/chat assistant',
        priority: 'HIGH',
        status: 'PENDING',
        dueAt: new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString(),
        source: 'CHAT'
      });
      onTaskCreated();
      addBotMessage(`✓ Action executed: Created task "${pendingToolCall.parameters.title}" & Google Calendar sync!`);
      speakText(`Task created successfully.`);
    } else if (pendingToolCall.toolName === 'createCalendarReminders') {
      onTaskCreated();
      addBotMessage(`✓ Action executed: Scheduled 3 reminders for Oracle Internship (8 Aug, 10 Aug, 11 Aug at 6 PM).`);
      speakText(`Scheduled 3 reminders for Oracle internship.`);
    }

    setPendingToolCall(null);
  };

  const handleRejectTool = () => {
    addBotMessage(`✕ Tool execution cancelled.`);
    setPendingToolCall(null);
  };

  return (
    <div className="glass-panel rounded-2xl border-2 border-cyan-500/40 overflow-hidden shadow-2xl transition-all">
      
      {/* Holographic Header Bar */}
      <div 
        onClick={() => setIsOpen(!isOpen)}
        className="flex items-center justify-between px-5 py-3.5 bg-gradient-to-r from-slate-950 via-slate-900 to-cyan-950/60 cursor-pointer border-b border-cyan-500/30"
      >
        <div className="flex items-center gap-3">
          <div className="w-9 h-9 rounded-xl bg-cyan-950 border border-cyan-400/60 flex items-center justify-center text-cyan-300 shadow-inner">
            <Terminal className="w-4.5 h-4.5 animate-pulse" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <span className="font-mono text-sm font-extrabold text-cyan-300 tracking-wider">JARVIS COMMAND TERMINAL</span>
              <span className="w-2 h-2 rounded-full bg-emerald-400 animate-ping" />
            </div>
            <p className="text-[11px] font-mono text-slate-400">
              AI SYSTEM OPERATIONAL // TOOL CALLING & VOICE AGENT ACTIVE
            </p>
          </div>
        </div>

        <div className="flex items-center gap-3" onClick={e => e.stopPropagation()}>
          
          {/* Soundwave animation when microphone active */}
          {isListening && (
            <div className="flex items-center gap-1 px-2.5 py-1 rounded-lg bg-cyan-950 border border-cyan-400">
              <div className="soundwave-bar" style={{ animationDelay: '0s' }} />
              <div className="soundwave-bar" style={{ animationDelay: '0.2s' }} />
              <div className="soundwave-bar" style={{ animationDelay: '0.4s' }} />
              <div className="soundwave-bar" style={{ animationDelay: '0.1s' }} />
              <span className="text-[10px] font-mono text-cyan-300 font-bold ml-1">LISTENING...</span>
            </div>
          )}

          <button
            onClick={() => setTtsEnabled(!ttsEnabled)}
            className={`p-1.5 rounded-lg border text-xs transition-colors ${
              ttsEnabled 
                ? 'bg-cyan-500/20 text-cyan-300 border-cyan-500/50' 
                : 'bg-slate-900 text-slate-500 border-slate-800'
            }`}
            title={ttsEnabled ? 'Disable Voice Speech Output' : 'Enable Voice Speech Output'}
          >
            {ttsEnabled ? <Volume2 className="w-4 h-4" /> : <VolumeX className="w-4 h-4" />}
          </button>

          <button 
            onClick={() => setIsOpen(!isOpen)}
            className="text-slate-400 hover:text-slate-200"
          >
            {isOpen ? <ChevronUp className="w-5 h-5" /> : <ChevronDown className="w-5 h-5" />}
          </button>
        </div>
      </div>

      {isOpen && (
        <div className="p-4 flex flex-col gap-4 bg-slate-950/60">
          
          {/* Quick Action Chips */}
          <div className="flex flex-wrap items-center gap-2 pb-2 border-b border-slate-800 text-xs">
            <span className="text-slate-400 text-[11px] font-mono font-bold mr-1">QUICK COMMANDS:</span>
            {[
              "What should I do today?",
              "Remind me about Oracle deadline",
              "Remind me to submit assignment tomorrow at 8 PM"
            ].map((chip, idx) => (
              <button
                key={idx}
                onClick={() => handleSendMessage(chip)}
                className="px-3 py-1 rounded-xl bg-slate-900/90 hover:bg-cyan-950 hover:text-cyan-200 border border-slate-800 hover:border-cyan-500/50 text-slate-300 font-mono text-xs transition-all shadow-sm"
              >
                {chip}
              </button>
            ))}
          </div>

          {/* Chat Messages Stream */}
          <div className="h-64 overflow-y-auto pr-2 space-y-3 font-sans text-sm">
            {messages.map((msg) => (
              <div
                key={msg.id}
                className={`flex gap-3 ${msg.sender === 'USER' ? 'justify-end' : 'justify-start'}`}
              >
                {msg.sender === 'SHARMAN' && (
                  <div className="w-8 h-8 rounded-xl bg-slate-950 border-2 border-cyan-400/60 flex items-center justify-center text-cyan-300 shrink-0 mt-0.5 shadow-md">
                    <Bot className="w-4.5 h-4.5" />
                  </div>
                )}

                <div
                  className={`max-w-[80%] rounded-2xl px-4 py-3 ${
                    msg.sender === 'USER'
                      ? 'bg-gradient-to-r from-cyan-600 via-sky-600 to-emerald-600 text-white rounded-tr-none shadow-lg shadow-cyan-600/20 font-medium'
                      : 'bg-slate-900/90 border border-slate-800 text-slate-100 rounded-tl-none whitespace-pre-line shadow-inner'
                  }`}
                >
                  <p className="text-xs leading-relaxed">{msg.text}</p>
                  <span className="text-[10px] opacity-60 mt-1 block text-right font-mono">
                    {msg.timestamp}
                  </span>
                </div>
              </div>
            ))}

            {/* Pending Tool Execution Confirmation Dialog */}
            {pendingToolCall && (
              <div className="p-4.5 rounded-2xl bg-gradient-to-r from-cyan-950/95 via-slate-900 to-slate-950 border-2 border-cyan-400 shadow-2xl shadow-cyan-500/30 space-y-3">
                <div className="flex items-center gap-2 text-cyan-300 font-mono text-xs uppercase font-extrabold tracking-wider">
                  <Sparkles className="w-4 h-4 text-cyan-400 animate-spin" />
                  CONFIRMATION REQUIRED — AI TOOL FUNCTION EXECUTION
                </div>
                
                <p className="text-sm text-slate-100 font-semibold">
                  {pendingToolCall.description}
                </p>

                <div className="flex items-center gap-3 pt-2">
                  <button
                    onClick={handleApproveTool}
                    className="flex-1 flex items-center justify-center gap-2 py-2.5 px-4 rounded-xl bg-gradient-to-r from-cyan-500 via-sky-400 to-emerald-400 hover:from-cyan-400 hover:to-emerald-300 text-slate-950 font-black text-xs shadow-lg shadow-cyan-500/30 transition-all uppercase tracking-wider"
                  >
                    <Check className="w-4.5 h-4.5 stroke-[3]" />
                    APPROVE & EXECUTE TOOL
                  </button>

                  <button
                    onClick={handleRejectTool}
                    className="flex items-center justify-center gap-2 py-2.5 px-4 rounded-xl bg-slate-900 hover:bg-slate-800 text-slate-300 border border-slate-800 text-xs font-semibold transition-all"
                  >
                    <X className="w-4 h-4" />
                    CANCEL
                  </button>
                </div>
              </div>
            )}

            <div ref={chatEndRef} />
          </div>

          {/* Input Box & Voice Trigger */}
          <div className="flex items-center gap-2 pt-2 border-t border-slate-800/80">
            <button
              onClick={toggleListening}
              className={`p-3 rounded-xl border transition-all ${
                isListening
                  ? 'bg-cyan-500/30 text-cyan-200 border-2 border-cyan-400 mic-active-pulse animate-pulse'
                  : 'bg-slate-900 hover:bg-slate-800 text-slate-400 border-slate-800'
              }`}
              title={isListening ? 'Listening... Click to stop' : 'Click to Speak (Speech-to-Text)'}
            >
              {isListening ? <Mic className="w-5 h-5 text-cyan-300" /> : <MicOff className="w-5 h-5" />}
            </button>

            <input
              type="text"
              value={inputText}
              onChange={e => setInputText(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && handleSendMessage()}
              placeholder="Ask SHARMAN: 'Remind me to submit assignment' or 'What should I do today?'..."
              className="flex-1 px-4 py-3 rounded-xl bg-slate-950 border border-slate-800 text-slate-100 placeholder-slate-500 text-sm focus:outline-none focus:border-cyan-500/70 shadow-inner font-sans"
            />

            <button
              onClick={() => handleSendMessage()}
              disabled={!inputText.trim()}
              className="p-3 rounded-xl bg-gradient-to-r from-cyan-500 to-emerald-500 hover:from-cyan-400 hover:to-emerald-400 disabled:opacity-40 text-slate-950 font-bold shadow-lg shadow-cyan-500/25 transition-all"
            >
              <Send className="w-5 h-5" />
            </button>
          </div>

        </div>
      )}
    </div>
  );
};
