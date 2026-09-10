package com.sharman.repository;

import com.sharman.model.InternshipOpportunity;
import org.springframework.data.jpa.repository.JpaRepository;
import org.springframework.stereotype.Repository;
import java.util.List;

@Repository
public interface OpportunityRepository extends JpaRepository<InternshipOpportunity, Long> {
    List<InternshipOpportunity> findBySavedTrue();
    List<InternshipOpportunity> findAllByOrderByMatchScoreDesc();
}
