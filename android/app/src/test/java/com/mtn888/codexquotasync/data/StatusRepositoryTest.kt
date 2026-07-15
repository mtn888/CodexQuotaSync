package com.mtn888.codexquotasync.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class StatusRepositoryTest {
    @Test
    fun `normalizes valid NAS base URLs`() {
        assertEquals(
            "http://nas.example.com:18080",
            StatusRepository.normalizeBaseUrl("  http://nas.example.com:18080/  "),
        )
        assertEquals(
            "http://192.168.1.20:8080/api/v1/status",
            StatusRepository.endpointFor("http://192.168.1.20:8080/api"),
        )
    }

    @Test
    fun `does not duplicate endpoint path`() {
        assertEquals(
            "http://nas.example.com/v1/status",
            StatusRepository.endpointFor("http://nas.example.com/v1/status"),
        )
    }

    @Test
    fun `rejects non HTTP and credential URLs`() {
        assertThrows(ConfigurationException::class.java) {
            StatusRepository.normalizeBaseUrl("file:///tmp/status.json")
        }
        assertThrows(ConfigurationException::class.java) {
            StatusRepository.normalizeBaseUrl("http://user:secret@example.com")
        }
    }
}
