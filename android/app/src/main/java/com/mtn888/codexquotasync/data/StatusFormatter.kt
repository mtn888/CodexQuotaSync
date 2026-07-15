package com.mtn888.codexquotasync.data

import java.time.Duration
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlin.math.roundToInt

object StatusFormatter {
    private val resetTimeFormat = DateTimeFormatter.ofPattern("M月d日 HH:mm")
    private val updateTimeFormat = DateTimeFormatter.ofPattern("M月d日 HH:mm")
    // WorkManager's 15-minute interval is a minimum, not a deadline. Leave
    // enough room for Doze and OEM scheduling jitter before calling data stale.
    private val staleAfter: Duration = Duration.ofMinutes(45)

    fun percentage(window: UsageWindow?): String =
        window?.let { "${it.remainingPercent.roundToInt()}%" } ?: "—"

    fun progress(window: UsageWindow?): Int =
        window?.remainingPercent?.roundToInt()?.coerceIn(0, 100) ?: 0

    fun nextReset(
        snapshot: ProviderSnapshot?,
        zoneId: ZoneId = ZoneId.systemDefault(),
        now: Instant = Instant.now(),
    ): String {
        snapshot ?: return "下次重置：—"
        val windowCandidates = listOf(
            "5h" to snapshot.shortWindow?.resetsAt,
            "weekly" to snapshot.weeklyWindow?.resetsAt,
        ).mapNotNull { (label, instant) -> instant?.let { label to it } }
        val selected = if (windowCandidates.isNotEmpty()) {
            windowCandidates.filter { (_, instant) -> instant > now }.minByOrNull { it.second }
        } else {
            snapshot.nextResetAt?.takeIf { it > now }?.let { snapshot.nextResetWindow to it }
        } ?: return "下次重置：—"
        val window = when (selected.first) {
            "5h" -> "5 小时额度"
            "weekly" -> "周额度"
            else -> "额度"
        }
        return "下次重置（$window）：${resetTimeFormat.withZone(zoneId).format(selected.second)}"
    }

    fun activity(activity: ActivityStatus): String =
        "执行中 ${activity.executing}   待处理 ${pending(activity)}"

    fun pending(activity: ActivityStatus): Int =
        activity.waitingOnApproval + activity.waitingOnUserInput

    fun updateTime(payload: StatusPayload, zoneId: ZoneId = ZoneId.systemDefault()): String =
        updateTimeFormat.withZone(zoneId).format(payload.collectedAt)

    fun isStale(payload: StatusPayload, now: Instant = Instant.now()): Boolean {
        val snapshot = payload.lastGoodSnapshot
        return payload.activity.stale ||
            payload.latestAttempt.status != "ok" ||
            snapshot?.status != "ok" ||
            Duration.between(payload.collectedAt, now) > staleAfter
    }
}
