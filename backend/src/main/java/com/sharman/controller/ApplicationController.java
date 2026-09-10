package com.sharman.controller;

import com.sharman.model.Application;
import com.sharman.repository.ApplicationRepository;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.*;

import java.util.List;

@RestController
@RequestMapping("/applications")
@CrossOrigin(origins = "*")
public class ApplicationController {

    @Autowired
    private ApplicationRepository applicationRepository;

    @GetMapping
    public List<Application> getApplications() {
        return applicationRepository.findAll();
    }

    @PostMapping
    public Application createApplication(@RequestBody Application application) {
        return applicationRepository.save(application);
    }

    @PutMapping("/{id}/stage")
    public ResponseEntity<Application> updateStage(@PathVariable Long id, @RequestParam Application.ApplicationStage stage) {
        return applicationRepository.findById(id)
                .map(app -> {
                    app.setStatus(stage);
                    return ResponseEntity.ok(applicationRepository.save(app));
                })
                .orElse(ResponseEntity.notFound().build());
    }
}
