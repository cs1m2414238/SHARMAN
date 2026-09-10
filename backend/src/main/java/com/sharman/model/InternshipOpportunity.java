package com.sharman.model;

import jakarta.persistence.*;
import java.time.LocalDate;

@Entity
@Table(name = "internship_opportunities")
public class InternshipOpportunity {

    @Id
    @GeneratedValue(strategy = GenerationType.IDENTITY)
    private Long id;

    private String company;
    private String role;
    private String location;
    private String skills;
    private String applicationUrl;
    private LocalDate deadline;
    private String source;

    private int matchScore;

    @Column(columnDefinition = "TEXT")
    private String matchReason;

    private boolean saved;

    public InternshipOpportunity() {}

    public InternshipOpportunity(Long id, String company, String role, String location, String skills, String applicationUrl, LocalDate deadline, String source, int matchScore, String matchReason, boolean saved) {
        this.id = id;
        this.company = company;
        this.role = role;
        this.location = location;
        this.skills = skills;
        this.applicationUrl = applicationUrl;
        this.deadline = deadline;
        this.source = source;
        this.matchScore = matchScore;
        this.matchReason = matchReason;
        this.saved = saved;
    }

    // Getters and Setters
    public Long getId() { return id; }
    public void setId(Long id) { this.id = id; }

    public String getCompany() { return company; }
    public void setCompany(String company) { this.company = company; }

    public String getRole() { return role; }
    public void setRole(String role) { this.role = role; }

    public String getLocation() { return location; }
    public void setLocation(String location) { this.location = location; }

    public String getSkills() { return skills; }
    public void setSkills(String skills) { this.skills = skills; }

    public String getApplicationUrl() { return applicationUrl; }
    public void setApplicationUrl(String applicationUrl) { this.applicationUrl = applicationUrl; }

    public LocalDate getDeadline() { return deadline; }
    public void setDeadline(LocalDate deadline) { this.deadline = deadline; }

    public String getSource() { return source; }
    public void setSource(String source) { this.source = source; }

    public int getMatchScore() { return matchScore; }
    public void setMatchScore(int matchScore) { this.matchScore = matchScore; }

    public String getMatchReason() { return matchReason; }
    public void setMatchReason(String matchReason) { this.matchReason = matchReason; }

    public boolean isSaved() { return saved; }
    public void setSaved(boolean saved) { this.saved = saved; }

    public static InternshipOpportunityBuilder builder() { return new InternshipOpportunityBuilder(); }

    public static class InternshipOpportunityBuilder {
        private Long id;
        private String company;
        private String role;
        private String location;
        private String skills;
        private String applicationUrl;
        private LocalDate deadline;
        private String source;
        private int matchScore;
        private String matchReason;
        private boolean saved;

        public InternshipOpportunityBuilder id(Long id) { this.id = id; return this; }
        public InternshipOpportunityBuilder company(String company) { this.company = company; return this; }
        public InternshipOpportunityBuilder role(String role) { this.role = role; return this; }
        public InternshipOpportunityBuilder location(String location) { this.location = location; return this; }
        public InternshipOpportunityBuilder skills(String skills) { this.skills = skills; return this; }
        public InternshipOpportunityBuilder applicationUrl(String applicationUrl) { this.applicationUrl = applicationUrl; return this; }
        public InternshipOpportunityBuilder deadline(LocalDate deadline) { this.deadline = deadline; return this; }
        public InternshipOpportunityBuilder source(String source) { this.source = source; return this; }
        public InternshipOpportunityBuilder matchScore(int matchScore) { this.matchScore = matchScore; return this; }
        public InternshipOpportunityBuilder matchReason(String matchReason) { this.matchReason = matchReason; return this; }
        public InternshipOpportunityBuilder saved(boolean saved) { this.saved = saved; return this; }

        public InternshipOpportunity build() {
            return new InternshipOpportunity(id, company, role, location, skills, applicationUrl, deadline, source, matchScore, matchReason, saved);
        }
    }
}
