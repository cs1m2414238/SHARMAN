package com.sharman.controller;

import com.sharman.model.InternshipOpportunity;
import com.sharman.repository.OpportunityRepository;
import com.sharman.service.OpportunityRankingService;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.web.bind.annotation.*;

import java.util.List;

@RestController
@RequestMapping("/opportunities")
@CrossOrigin(origins = "*")
public class OpportunityController {

    @Autowired
    private OpportunityRepository opportunityRepository;

    @Autowired
    private OpportunityRankingService rankingService;

    @GetMapping
    public List<InternshipOpportunity> getOpportunities() {
        List<InternshipOpportunity> opps = opportunityRepository.findAll();
        for (InternshipOpportunity opp : opps) {
            int score = rankingService.calculateMatchScore(opp);
            opp.setMatchScore(score);
            opp.setMatchReason(rankingService.generateMatchReason(opp, score));
        }
        return opps;
    }

    @PostMapping
    public InternshipOpportunity saveOpportunity(@RequestBody InternshipOpportunity opportunity) {
        int score = rankingService.calculateMatchScore(opportunity);
        opportunity.setMatchScore(score);
        opportunity.setMatchReason(rankingService.generateMatchReason(opportunity, score));
        return opportunityRepository.save(opportunity);
    }
}
