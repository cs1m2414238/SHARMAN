package com.sharman;

import com.sharman.model.InternshipOpportunity;
import com.sharman.service.OpportunityRankingService;
import org.junit.jupiter.api.Test;
import java.time.LocalDate;
import static org.junit.jupiter.api.Assertions.*;

public class OpportunityRankingServiceTest {

    @Test
    public void testOracleMatchScore() {
        OpportunityRankingService rankingService = new OpportunityRankingService();

        InternshipOpportunity oracleOpp = InternshipOpportunity.builder()
                .company("Oracle")
                .role("Software Engineering Internship")
                .skills("Java, SQL, Spring Boot")
                .location("Bengaluru")
                .deadline(LocalDate.now().plusDays(5))
                .build();

        int score = rankingService.calculateMatchScore(oracleOpp);
        assertTrue(score >= 80, "Oracle Java/Spring Boot internship should have high match score >= 80%");
    }
}
