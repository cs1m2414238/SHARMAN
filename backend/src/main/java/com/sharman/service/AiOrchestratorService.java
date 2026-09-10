package com.sharman.service;

import com.sharman.model.Task;
import com.sharman.repository.TaskRepository;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.stereotype.Service;

import java.time.LocalDateTime;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

@Service
public class AiOrchestratorService {

    @Autowired
    private TaskRepository taskRepository;

    public Map<String, Object> executeTool(String toolName, Map<String, Object> parameters) {
        Map<String, Object> response = new HashMap<>();

        switch (toolName) {
            case "createTask":
                String title = (String) parameters.get("title");
                String priorityStr = (String) parameters.getOrDefault("priority", "MEDIUM");
                Task task = Task.builder()
                        .title(title)
                        .description((String) parameters.get("description"))
                        .priority(Task.Priority.valueOf(priorityStr.toUpperCase()))
                        .status(Task.TaskStatus.PENDING)
                        .dueAt(LocalDateTime.now().plusDays(1))
                        .source("VOICE_CHAT")
                        .build();
                Task savedTask = taskRepository.save(task);
                response.put("success", true);
                response.put("task", savedTask);
                response.put("message", "Created task: " + title);
                break;

            case "listTodayTasks":
                List<Task> tasks = taskRepository.findAll();
                response.put("success", true);
                response.put("tasks", tasks);
                response.put("count", tasks.size());
                break;

            case "generateDailyBrief":
                response.put("success", true);
                response.put("briefing", "Good morning! You have 3 pending tasks: Complete Java assignment due tomorrow, apply to Oracle Software Engineering Internship, and finish Spring Boot REST API.");
                break;

            default:
                response.put("success", false);
                response.put("message", "Unknown tool: " + toolName);
        }

        return response;
    }
}
