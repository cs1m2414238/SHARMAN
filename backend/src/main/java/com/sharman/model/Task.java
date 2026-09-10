package com.sharman.model;

import jakarta.persistence.*;
import java.time.LocalDateTime;

@Entity
@Table(name = "tasks")
public class Task {

    @Id
    @GeneratedValue(strategy = GenerationType.IDENTITY)
    private Long id;

    private Long userId;

    @Column(nullable = false)
    private String title;

    @Column(columnDefinition = "TEXT")
    private String description;

    @Enumerated(EnumType.STRING)
    private Priority priority;

    @Enumerated(EnumType.STRING)
    private TaskStatus status;

    private LocalDateTime dueAt;

    private String source;

    private LocalDateTime createdAt;
    private LocalDateTime updatedAt;

    public Task() {}

    public Task(Long id, Long userId, String title, String description, Priority priority, TaskStatus status, LocalDateTime dueAt, String source, LocalDateTime createdAt, LocalDateTime updatedAt) {
        this.id = id;
        this.userId = userId;
        this.title = title;
        this.description = description;
        this.priority = priority;
        this.status = status;
        this.dueAt = dueAt;
        this.source = source;
        this.createdAt = createdAt;
        this.updatedAt = updatedAt;
    }

    @PrePersist
    protected void onCreate() {
        createdAt = LocalDateTime.now();
        updatedAt = LocalDateTime.now();
        if (status == null) status = TaskStatus.PENDING;
        if (priority == null) priority = Priority.MEDIUM;
    }

    @PreUpdate
    protected void onUpdate() {
        updatedAt = LocalDateTime.now();
    }

    public enum Priority { HIGH, MEDIUM, LOW }
    public enum TaskStatus { PENDING, COMPLETED, OVERDUE }

    // Getters and Setters
    public Long getId() { return id; }
    public void setId(Long id) { this.id = id; }

    public Long getUserId() { return userId; }
    public void setUserId(Long userId) { this.userId = userId; }

    public String getTitle() { return title; }
    public void setTitle(String title) { this.title = title; }

    public String getDescription() { return description; }
    public void setDescription(String description) { this.description = description; }

    public Priority getPriority() { return priority; }
    public void setPriority(Priority priority) { this.priority = priority; }

    public TaskStatus getStatus() { return status; }
    public void setStatus(TaskStatus status) { this.status = status; }

    public LocalDateTime getDueAt() { return dueAt; }
    public void setDueAt(LocalDateTime dueAt) { this.dueAt = dueAt; }

    public String getSource() { return source; }
    public void setSource(String source) { this.source = source; }

    public LocalDateTime getCreatedAt() { return createdAt; }
    public void setCreatedAt(LocalDateTime createdAt) { this.createdAt = createdAt; }

    public LocalDateTime getUpdatedAt() { return updatedAt; }
    public void setUpdatedAt(LocalDateTime updatedAt) { this.updatedAt = updatedAt; }

    // Builder pattern implementation
    public static TaskBuilder builder() { return new TaskBuilder(); }

    public static class TaskBuilder {
        private Long id;
        private Long userId;
        private String title;
        private String description;
        private Priority priority;
        private TaskStatus status;
        private LocalDateTime dueAt;
        private String source;

        public TaskBuilder id(Long id) { this.id = id; return this; }
        public TaskBuilder userId(Long userId) { this.userId = userId; return this; }
        public TaskBuilder title(String title) { this.title = title; return this; }
        public TaskBuilder description(String description) { this.description = description; return this; }
        public TaskBuilder priority(Priority priority) { this.priority = priority; return this; }
        public TaskBuilder status(TaskStatus status) { this.status = status; return this; }
        public TaskBuilder dueAt(LocalDateTime dueAt) { this.dueAt = dueAt; return this; }
        public TaskBuilder source(String source) { this.source = source; return this; }

        public Task build() {
            return new Task(id, userId, title, description, priority, status, dueAt, source, LocalDateTime.now(), LocalDateTime.now());
        }
    }
}
