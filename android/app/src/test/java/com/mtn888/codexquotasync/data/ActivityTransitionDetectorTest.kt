package com.mtn888.codexquotasync.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertNotNull
import org.junit.Test
import java.time.Instant

class ActivityTransitionDetectorTest {
    @Test
    fun `first successful fetch only establishes baseline`() {
        assertNull(ActivityTransitionDetector.detect(null, payload(1, 2, 0, 0)))
    }

    @Test
    fun `detects completed task and increased merged pending count`() {
        val transition = ActivityTransitionDetector.detect(
            payload(1, 3, 1, 0),
            payload(2, 1, 1, 2),
        )

        assertNotNull(transition)
        assertEquals(2, transition?.completed)
        assertEquals(2, transition?.pendingIncrease)
        assertEquals(3, transition?.currentPending)
    }

    @Test
    fun `detects only a completed task`() {
        val transition = ActivityTransitionDetector.detect(
            payload(1, 2, 0, 0),
            payload(2, 1, 0, 0),
        )

        assertEquals(1, transition?.completed)
        assertEquals(0, transition?.pendingIncrease)
    }

    @Test
    fun `detects only an increase in merged pending count`() {
        val transition = ActivityTransitionDetector.detect(
            payload(1, 1, 0, 0),
            payload(2, 1, 1, 1),
        )

        assertEquals(0, transition?.completed)
        assertEquals(2, transition?.pendingIncrease)
    }

    @Test
    fun `approval and input redistribution does not notify`() {
        assertNull(
            ActivityTransitionDetector.detect(
                payload(1, 1, 2, 0),
                payload(2, 1, 0, 2),
            ),
        )
    }

    @Test
    fun `increases in executing and decreases in pending do not notify`() {
        assertNull(
            ActivityTransitionDetector.detect(
                payload(1, 1, 2, 1),
                payload(2, 3, 1, 0),
            ),
        )
    }

    @Test
    fun `stale unavailable changed source and non increasing revisions do not notify`() {
        val baseline = payload(5, 2, 0, 0)
        assertNull(ActivityTransitionDetector.detect(baseline, payload(5, 0, 1, 0)))
        assertNull(ActivityTransitionDetector.detect(baseline, payload(6, 0, 1, 0, stale = true)))
        assertNull(ActivityTransitionDetector.detect(baseline, payload(6, 0, 1, 0, activitySource = "unavailable")))
        assertNull(ActivityTransitionDetector.detect(baseline, payload(6, 0, 1, 0, sourceId = "other")))
    }

    private fun payload(
        revision: Long,
        executing: Int,
        approval: Int,
        input: Int,
        stale: Boolean = false,
        activitySource: String = "hooks",
        sourceId: String = "windows-main",
    ): StatusPayload {
        val at = Instant.parse("2026-07-15T12:00:00Z")
        return StatusPayload(
            sourceId = sourceId,
            revision = revision,
            collectorVersion = "test",
            collectedAt = at,
            receivedAt = at,
            activity = ActivityStatus(executing, approval, input, activitySource, at, stale),
            latestAttempt = AttemptStatus("ok", null, at),
            lastGoodSnapshot = null,
        )
    }
}
