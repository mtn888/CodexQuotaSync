package com.mtn888.codexquotasync.data

import android.content.Context
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URI
import java.net.URL
import java.time.Instant

class StatusRepository(private val context: Context) {
    private val configPreferences = context.getSharedPreferences(CONFIG_PREFERENCES, Context.MODE_PRIVATE)
    private val cachePreferences = context.getSharedPreferences(CACHE_PREFERENCES, Context.MODE_PRIVATE)

    fun configuredBaseUrl(): String? = configPreferences.getString(KEY_BASE_URL, null)

    fun saveBaseUrl(input: String): String {
        val normalized = normalizeBaseUrl(input)
        configPreferences.edit().putString(KEY_BASE_URL, normalized).apply()
        return normalized
    }

    fun fetch(): StatusPayload {
        val baseUrl = configuredBaseUrl() ?: throw ConfigurationException("尚未配置服务器地址")
        val endpoint = endpointFor(baseUrl)
        val connection = (URL(endpoint).openConnection() as? HttpURLConnection)
            ?: throw IOException("服务器地址不是 HTTP URL")

        try {
            connection.requestMethod = "GET"
            connection.connectTimeout = CONNECT_TIMEOUT_MILLIS
            connection.readTimeout = READ_TIMEOUT_MILLIS
            connection.instanceFollowRedirects = false
            connection.setRequestProperty("Accept", "application/json")
            connection.setRequestProperty("User-Agent", "Codex-Quota-Sync-Android/0.1.0")

            val statusCode = connection.responseCode
            if (statusCode !in 200..299) {
                throw IOException("服务器返回 HTTP $statusCode")
            }

            val json = connection.inputStream.use(::readLimitedUtf8)
            val parsed = StatusParser.parse(json)
            cachePreferences.edit()
                .putString(KEY_LAST_JSON, json)
                .putLong(KEY_LAST_SUCCESS_AT, System.currentTimeMillis())
                .apply()
            return parsed
        } finally {
            connection.disconnect()
        }
    }

    fun loadCached(): CachedStatus? {
        val json = cachePreferences.getString(KEY_LAST_JSON, null) ?: return null
        return try {
            CachedStatus(
                payload = StatusParser.parse(json),
                fetchedAt = Instant.ofEpochMilli(cachePreferences.getLong(KEY_LAST_SUCCESS_AT, 0L)),
            )
        } catch (_: Exception) {
            cachePreferences.edit().clear().apply()
            null
        }
    }

    companion object {
        private const val CONFIG_PREFERENCES = "codex_quota_config"
        private const val CACHE_PREFERENCES = "codex_quota_cache"
        private const val KEY_BASE_URL = "base_url"
        private const val KEY_LAST_JSON = "last_json"
        private const val KEY_LAST_SUCCESS_AT = "last_success_at"
        private const val CONNECT_TIMEOUT_MILLIS = 10_000
        private const val READ_TIMEOUT_MILLIS = 10_000
        private const val MAX_RESPONSE_CHARS = 512 * 1024

        fun normalizeBaseUrl(input: String): String {
            val trimmed = input.trim().trimEnd('/')
            if (trimmed.isEmpty()) throw ConfigurationException("请输入服务器地址")
            val uri = try {
                URI(trimmed)
            } catch (error: Exception) {
                throw ConfigurationException("服务器地址格式不正确", error)
            }
            if (uri.scheme?.lowercase() !in setOf("http", "https")) {
                throw ConfigurationException("地址必须以 http:// 或 https:// 开头")
            }
            if (uri.host.isNullOrBlank()) throw ConfigurationException("服务器地址缺少主机名或 IP")
            if (uri.userInfo != null) throw ConfigurationException("服务器地址不能包含用户名或密码")
            if (uri.query != null || uri.fragment != null) {
                throw ConfigurationException("Base URL 不能包含查询参数或 #fragment")
            }
            return uri.toASCIIString().trimEnd('/')
        }

        fun endpointFor(baseUrl: String): String {
            val normalized = normalizeBaseUrl(baseUrl)
            return if (normalized.endsWith("/v1/status")) normalized else "$normalized/v1/status"
        }

        private fun readLimitedUtf8(stream: java.io.InputStream): String {
            val output = StringBuilder()
            stream.bufferedReader(Charsets.UTF_8).use { reader ->
                val buffer = CharArray(8 * 1024)
                while (true) {
                    val count = reader.read(buffer)
                    if (count < 0) break
                    output.append(buffer, 0, count)
                    if (output.length > MAX_RESPONSE_CHARS) {
                        throw IOException("服务器响应超过 512 KiB 限制")
                    }
                }
            }
            return output.toString()
        }
    }
}

data class CachedStatus(val payload: StatusPayload, val fetchedAt: Instant)

class ConfigurationException(message: String, cause: Throwable? = null) : Exception(message, cause)
