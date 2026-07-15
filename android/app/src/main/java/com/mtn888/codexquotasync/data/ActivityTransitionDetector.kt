package com.mtn888.codexquotasync.data

import kotlin.math.max

data class ActivityTransition(
    val completed: Int,
    val pendingIncrease: Int,
    val previousExecuting: Int,
    val currentExecuting: Int,
    val previousPending: Int,
    val currentPending: Int,
)

object ActivityTransitionDetector {
    fun detect(previous: StatusPayload?, current: StatusPayload): ActivityTransition? {
        previous ?: return null
        if (previous.sourceId != current.sourceId || current.revision <= previous.revision) return null
        if (!previous.activity.isReliable() || !current.activity.isReliable()) return null

        val previousPending = StatusFormatter.pending(previous.activity)
        val currentPending = StatusFormatter.pending(current.activity)
        val completed = max(previous.activity.executing - current.activity.executing, 0)
        val pendingIncrease = max(currentPending - previousPending, 0)
        if (completed == 0 && pendingIncrease == 0) return null

        return ActivityTransition(
            completed = completed,
            pendingIncrease = pendingIncrease,
            previousExecuting = previous.activity.executing,
            currentExecuting = current.activity.executing,
            previousPending = previousPending,
            currentPending = currentPending,
        )
    }

    private fun ActivityStatus.isReliable(): Boolean = source == "hooks" && !stale
}
