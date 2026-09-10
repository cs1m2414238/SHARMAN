package com.sharman.service;

import com.sharman.model.InternshipOpportunity;
import org.springframework.stereotype.Service;
import java.time.LocalDate;
import java.time.temporal.ChronoUnit;
import java.util.Arrays;
import java.util.List;
import java.util.Set;
import java.util.stream.Collectors;

@Service
public class OpportunityRankingService {

    // User Profile for matching (Default student context)
    private static final Set<String> USER_SKILLS = Set.of("java", "sql", "spring boot", "react", "typescript", "python");
    private static final String PREFERRED_LOCATION = "bengaluru";
    private static final boolean PREFER_REMOTE = true;
    private static final Set<String> TARGET_COMPANIES = Set.of("oracle", "google", "microsoft", "amazon", "atlassian", "uber");

    public int calculateMatchScore(InternshipOpportunity opportunity) {
        double totalScore = 0.0;

        // 1. Skill match (35%)
        double skillScore = computeSkillScore(opportunity.getSkills());
        totalScore += skillScore * 0.35;

        // 2. Student eligibility (20%) - Assumes 2025/2026 graduate eligibility
        double eligibilityScore = 1.0; // 100% eligible for standard internships
        totalScore += eligibilityScore * 0.20;

        // 3. Location / Remote (15%)
        double locationScore = computeLocationScore(opportunity.getLocation());
        totalScore += locationScore * 0.15;

        // 4. Application deadline (15%)
        double deadlineScore = computeDeadlineScore(opportunity.getDeadline());
        totalScore += deadlineScore * 0.15;

        // 5. Company relevance (10%)
        double companyScore = computeCompanyScore(opportunity.getCompany());
        totalScore += companyScore * 0.10;

        // 6. Experience required (5%) - Entry level / Student intern gets 100%
        double expScore = 1.0;
        totalScore += expScore * 0.05;

        return (int) Math.round(totalScore * 100);
    }

    public String generateMatchReason(InternshipOpportunity opportunity, int score) {
        StringBuilder reason = new StringBuilder();
        if (score >= 80) {
            reason.append("Strong match with high skill compatibility in ").append(opportunity.getSkills());
        } else if (score >= 60) {
            reason.append("Moderate match based on student eligibility and location preference.");
        } else {
            reason.append("Low skill overlap; requires additional preparation.");
        }
        return reason.toString();
    }

    private double computeSkillScore(String opportunitySkillsStr) {
        if (opportunitySkillsStr == null || opportunitySkillsStr.isBlank()) return 0.5;
        List<String> oppSkills = Arrays.stream(opportunitySkillsStr.split(","))
                .map(s -> s.trim().toLowerCase())
                .collect(Collectors.toList());
        if (oppSkills.isEmpty()) return 0.5;

        long matched = oppSkills.stream().filter(USER_SKILLS::contains).count();
        return (double) matched / oppSkills.size();
    }

    private double computeLocationScore(String location) {
        if (location == null) return 0.5;
        String locLower = location.toLowerCase();
        if (locLower.contains("remote") || locLower.contains("hybrid")) return 1.0;
        if (locLower.contains(PREFERRED_LOCATION)) return 0.9;
        return 0.6;
    }

    private double computeDeadlineScore(LocalDate deadline) {
        if (deadline == null) return 0.7;
        long daysUntil = ChronoUnit.DAYS.between(LocalDate.now(), deadline);
        if (daysUntil < 0) return 0.0; // Expired
        if (daysUntil <= 3) return 1.0; // Urgent high priority
        if (daysUntil <= 14) return 0.85;
        return 0.7;
    }

    private double computeCompanyScore(String company) {
        if (company == null) return 0.5;
        if (TARGET_COMPANIES.contains(company.toLowerCase())) return 1.0;
        return 0.75;
    }
}
