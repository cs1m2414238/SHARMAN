package com.sharman.model;

import jakarta.persistence.*;
import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;
import java.time.LocalDateTime;

@Entity
@Table(name = "email_insights")
@Data
@NoArgsConstructor
@AllArgsConstructor
@Builder
public class EmailInsight {

    @Id
    @GeneratedValue(strategy = GenerationType.IDENTITY)
    private Long id;

    private Long userId;

    @Column(unique = true)
    private String gmailMessageId;

    private String subject;
    private String sender;

    private String category; // "ASSESSMENT", "INTERVIEW", "CONFIRMATION", "REJECTION", "DEADLINE"

    private String company;
    private String role;

    private LocalDateTime extractedDeadline;

    @Column(columnDefinition = "TEXT")
    private String requiredAction;

    private double confidenceScore; // e.g. 0.95

    private boolean processed;

    private LocalDateTime receivedAt;
}
