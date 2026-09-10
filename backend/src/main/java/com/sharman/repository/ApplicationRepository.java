package com.sharman.repository;

import com.sharman.model.Application;
import org.springframework.data.jpa.repository.JpaRepository;
import org.springframework.stereotype.Repository;
import java.util.List;

@Repository
public interface ApplicationRepository extends JpaRepository<Application, Long> {
    List<Application> findByStatus(Application.ApplicationStage status);
    List<Application> findByCompanyAndRole(String company, String role);
}
