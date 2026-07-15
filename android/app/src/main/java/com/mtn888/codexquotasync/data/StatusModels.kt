package com.mtn888.codexquotasync.data

import java.time.Instant

data class UsageWindow(
    val remainingPercent: Double,
    val resetsAt: Instant?,
    val windowSeconds: Long?,
)

data class ActivityStatus(
    val executing: Int,
    val waitingOnApproval: Int,
    val waitingOnUserInput: Int,
    val source: String,
    val observedAt: Instant,
    val stale: Boolean,
)

data class AttemptStatus(
    val status: String,
    val message: String?,
    val attemptedAt: Instant,
)

data class ProviderSnapshot(
    val displayName: String,
    val plan: String?,
    val shortWindow: UsageWindow?,
    val weeklyWindow: UsageWindow?,
    val updatedAt: Instant,
    val status: String,
    val message: String?,
    val nextResetAt: Instant?,
    val nextResetWindow: String?,
)

data class StatusPayload(
    val sourceId: String,
    val revision: Long,
    val collectorVersion: String,
    val collectedAt: Instant,
    val receivedAt: Instant?,
    val activity: ActivityStatus,
    val latestAttempt: AttemptStatus,
    val lastGoodSnapshot: ProviderSnapshot?,
)
