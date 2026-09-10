package com.sharman.model;

import jakarta.persistence.*;
import java.time.LocalDate;
import java.time.LocalDateTime;

@Entity
@Table(name = "applications")
public class Application {

    @Id
    @GeneratedValue(strategy = GenerationType.IDENTITY)
    private Long id;

    private Long userId;
    private Long opportunityId;

    private String company;
    private String role;
    private String location;

    @Enumerated(EnumType.STRING)
    private ApplicationStage status;

    private LocalDate appliedAt;
    private String nextAction;
    private LocalDate followUpAt;

    @Column(columnDefinition = "TEXT")
    private String notes;

    private LocalDateTime createdAt;
    private LocalDateTime updatedAt;

    public Application() {}

    public Application(Long id, Long userId, Long opportunityId, String company, String role, String location, ApplicationStage status, LocalDate appliedAt, String nextAction, LocalDate followUpAt, String notes, LocalDateTime createdAt, LocalDateTime updatedAt) {
        this.id = id;
        this.userId = userId;
        this.opportunityId = opportunityId;
        this.company = company;
        this.role = role;
        this.location = location;
        this.status = status;
        this.appliedAt = appliedAt;
        this.nextAction = nextAction;
        this.followUpAt = followUpAt;
        this.notes = notes;
        this.createdAt = createdAt;
        this.updatedAt = updatedAt;
    }

    @PrePersist
    protected void onCreate() {
        createdAt = LocalDateTime.now();
        updatedAt = LocalDateTime.now();
        if (status == null) status = ApplicationStage.DISCOVERED;
    }

    @PreUpdate
    protected void onUpdate() {
        updatedAt = LocalDateTime.now();
    }

    public enum ApplicationStage {
        DISCOVERED, SAVED, APPLIED, ASSESSMENT, INTERVIEW, OFFER, REJECTED
    }

    // Getters and Setters
    public Long getId() { return id; }
    public void setId(Long id) { this.id = id; }

    public Long getUserId() { return userId; }
    public void setUserId(Long userId) { this.userId = userId; }

    public Long getOpportunityId() { return opportunityId; }
    public void setOpportunityId(Long opportunityId) { this.opportunityId = opportunityId; }

    public String getCompany() { return company; }
    public void setCompany(String company) { this.company = company; }

    public String getRole() { return role; }
    public void setRole(String role) { this.role = role; }

    public String getLocation() { return location; }
    public void setLocation(String location) { this.location = location; }

    public ApplicationStage getStatus() { return status; }
    public void setStatus(ApplicationStage status) { this.status = status; }

    public LocalDate getAppliedAt() { return appliedAt; }
    public void setAppliedAt(LocalDate appliedAt) { this.appliedAt = appliedAt; }

    public String getNextAction() { return nextAction; }
    public void setNextAction(String nextAction) { this.nextAction = nextAction; }

    public LocalDate getFollowUpAt() { return followUpAt; }
    public void setFollowUpAt(LocalDate followUpAt) { this.followUpAt = followUpAt; }

    public String getNotes() { return notes; }
    public void setNotes(String notes) { this.notes = notes; }

    public LocalDateTime getCreatedAt() { return createdAt; }
    public void setCreatedAt(LocalDateTime createdAt) { this.createdAt = createdAt; }

    public LocalDateTime getUpdatedAt() { return updatedAt; }
    public void setUpdatedAt(LocalDateTime updatedAt) { this.updatedAt = updatedAt; }

    public static ApplicationBuilder builder() { return new ApplicationBuilder(); }

    public static class ApplicationBuilder {
        private Long id;
        private Long userId;
        private Long opportunityId;
        private String company;
        private String role;
        private String location;
        private ApplicationStage status;
        private LocalDate appliedAt;
        private String nextAction;
        private LocalDate followUpAt;
        private String notes;

        public ApplicationBuilder id(Long id) { this.id = id; return this; }
        public ApplicationBuilder userId(Long userId) { this.userId = userId; return this; }
        public ApplicationBuilder opportunityId(Long opportunityId) { this.opportunityId = opportunityId; return this; }
        public ApplicationBuilder company(String company) { this.company = company; return this; }
        public ApplicationBuilder role(String role) { this.role = role; return this; }
        public ApplicationBuilder location(String location) { this.location = location; return this; }
        public ApplicationBuilder status(ApplicationStage status) { this.status = status; return this; }
        public ApplicationBuilder appliedAt(LocalDate appliedAt) { this.appliedAt = appliedAt; return this; }
        public ApplicationBuilder nextAction(String nextAction) { this.nextAction = nextAction; return this; }
        public ApplicationBuilder followUpAt(LocalDate followUpAt) { this.followUpAt = followUpAt; return this; }
        public ApplicationBuilder notes(String notes) { this.notes = notes; return this; }

        public Application build() {
            return new Application(id, userId, opportunityId, company, role, location, status, appliedAt, nextAction, followUpAt, notes, LocalDateTime.now(), LocalDateTime.now());
        }
    }
}
