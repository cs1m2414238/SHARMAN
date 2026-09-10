package com.sharman.controller;

import com.sharman.service.AiOrchestratorService;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.web.bind.annotation.*;

import java.util.Map;

@RestController
@RequestMapping("/ai")
@CrossOrigin(origins = "*")
public class AiAssistantController {

    @Autowired
    private AiOrchestratorService aiOrchestratorService;

    @PostMapping("/tool-call")
    public Map<String, Object> executeTool(@RequestBody Map<String, Object> request) {
        String toolName = (String) request.get("toolName");
        @SuppressWarnings("unchecked")
        Map<String, Object> parameters = (Map<String, Object>) request.get("parameters");
        return aiOrchestratorService.executeTool(toolName, parameters);
    }
}
