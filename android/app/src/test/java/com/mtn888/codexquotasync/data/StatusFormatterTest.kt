package com.mtn888.codexquotasync.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Instant
import java.time.ZoneId

class StatusFormatterTest {
    @Test
    fun `formats percentage reset and activity`() {
        val snapshot = ProviderSnapshot(
            displayName = "Codex",
            plan = "Plus",
            shortWindow = null,
            weeklyWindow = null,
            updatedAt = Instant.parse("2026-07-15T12:00:00Z"),
            status = "ok",
            message = null,
            nextResetAt = Instant.parse("2026-07-15T20:00:00Z"),
            nextResetWindow = "5h",
        )
        val activity = ActivityStatus(2, 1, 3, "hooks", Instant.EPOCH, false)

        assertEquals("77%", StatusFormatter.percentage(UsageWindow(76.6, null, null)))
        assertEquals(
            "下次重置（5 小时额度）：7月15日 20:00",
            StatusFormatter.nextReset(
                snapshot,
                ZoneId.of("UTC"),
                Instant.parse("2026-07-15T12:00:00Z"),
            ),
        )
        assertEquals("执行中 2   待审批 1   待输入 3", StatusFormatter.activity(activity))
    }

    @Test
    fun `derives a future reset and hides an expired reset`() {
        val future = Instant.parse("2026-07-16T08:00:00Z")
        val past = Instant.parse("2026-07-15T08:00:00Z")
        val now = Instant.parse("2026-07-15T12:00:00Z")
        val snapshot = ProviderSnapshot(
            displayName = "Codex",
            plan = null,
            shortWindow = UsageWindow(50.0, past, 18_000),
            weeklyWindow = UsageWindow(40.0, future, 604_800),
            updatedAt = now,
            status = "ok",
            message = null,
            nextResetAt = past,
            nextResetWindow = "5h",
        )

        assertEquals(
            "下次重置（周额度）：7月16日 08:00",
            StatusFormatter.nextReset(snapshot, ZoneId.of("UTC"), now),
        )
        assertEquals(
            "下次重置：—",
            StatusFormatter.nextReset(
                snapshot.copy(weeklyWindow = null),
                ZoneId.of("UTC"),
                now,
            ),
        )
    }

    @Test
    fun `marks data stale from flags or age`() {
        val fresh = payload(
            collectedAt = Instant.parse("2026-07-15T12:00:00Z"),
            activityStale = false,
        )
        assertFalse(StatusFormatter.isStale(fresh, Instant.parse("2026-07-15T12:19:59Z")))
        assertTrue(StatusFormatter.isStale(fresh, Instant.parse("2026-07-15T12:20:01Z")))
        assertTrue(StatusFormatter.isStale(payload(Instant.parse("2026-07-15T12:00:00Z"), true), Instant.parse("2026-07-15T12:01:00Z")))
    }

    private fun payload(collectedAt: Instant, activityStale: Boolean): StatusPayload {
        return StatusPayload(
            sourceId = "pc",
            revision = 1,
            collectorVersion = "test",
            collectedAt = collectedAt,
            receivedAt = null,
            activity = ActivityStatus(0, 0, 0, "hooks", collectedAt, activityStale),
            latestAttempt = AttemptStatus("ok", null, collectedAt),
            lastGoodSnapshot = ProviderSnapshot(
                "Codex", null, null, null, collectedAt, "ok", null, null, null,
            ),
        )
    }
}
