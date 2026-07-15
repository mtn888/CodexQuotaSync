package com.mtn888.codexquotasync.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test
import java.time.Instant

class StatusParserTest {
    @Test
    fun `parses status v1 and all activity counters`() {
        val status = StatusParser.parse(validJson)

        assertEquals("main-pc", status.sourceId)
        assertEquals(42L, status.revision)
        assertEquals(2, status.activity.executing)
        assertEquals(1, status.activity.waitingOnApproval)
        assertEquals(3, status.activity.waitingOnUserInput)
        assertEquals(76.4, status.lastGoodSnapshot?.shortWindow?.remainingPercent ?: -1.0, 0.001)
        assertEquals(Instant.parse("2026-07-15T20:00:00Z"), status.lastGoodSnapshot?.nextResetAt)
        assertEquals("5h", status.lastGoodSnapshot?.nextResetWindow)
    }

    @Test
    fun `accepts null last good snapshot`() {
        val json = validJson.replace(snapshotJson, "null")

        assertNull(StatusParser.parse(json).lastGoodSnapshot)
    }

    @Test
    fun `rejects unsupported schema version`() {
        val json = validJson.replace("\"schemaVersion\": 1", "\"schemaVersion\": 2")

        assertThrows(StatusParseException::class.java) { StatusParser.parse(json) }
    }

    @Test
    fun `rejects quota percentage outside schema bounds`() {
        val json = validJson.replace("\"remainingPercent\": 76.4", "\"remainingPercent\": 101")

        assertThrows(StatusParseException::class.java) { StatusParser.parse(json) }
    }

    @Test
    fun `accepts optional latest attempt message being omitted`() {
        val json = validJson.replace("\n    \"message\": null,", "")

        assertNull(StatusParser.parse(json).latestAttempt.message)
    }

    companion object {
        private val snapshotJson = """
            {
              "provider": "codex",
              "displayName": "Codex",
              "plan": "Plus",
              "shortWindow": {
                "remainingPercent": 76.4,
                "resetsAt": "2026-07-15T20:00:00Z",
                "windowSeconds": 18000
              },
              "weeklyWindow": {
                "remainingPercent": 54,
                "resetsAt": "2026-07-20T00:00:00Z",
                "windowSeconds": 604800
              },
              "resetCredits": null,
              "resetCreditExpiresAt": [],
              "updatedAt": "2026-07-15T12:00:00Z",
              "status": "ok",
              "message": null,
              "nextResetAt": "2026-07-15T20:00:00Z",
              "nextResetWindow": "5h"
            }
        """.trimIndent()

        val validJson = """
            {
              "schemaVersion": 1,
              "sourceId": "main-pc",
              "revision": 42,
              "collectorVersion": "0.1.0",
              "collectedAt": "2026-07-15T12:00:00Z",
              "receivedAt": "2026-07-15T12:00:01Z",
              "activity": {
                "executing": 2,
                "waitingOnApproval": 1,
                "waitingOnUserInput": 3,
                "source": "hooks",
                "observedAt": "2026-07-15T12:00:00Z",
                "stale": false
              },
              "latestAttempt": {
                "status": "ok",
                "message": null,
                "attemptedAt": "2026-07-15T12:00:00Z"
              },
              "lastGoodSnapshot": $snapshotJson
            }
        """.trimIndent()
    }
}
