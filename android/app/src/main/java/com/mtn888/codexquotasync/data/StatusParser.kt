package com.mtn888.codexquotasync.data

import org.json.JSONArray
import org.json.JSONObject
import java.time.Instant
import java.time.format.DateTimeParseException

object StatusParser {
    private val allowedStatuses = setOf("ok", "stale", "signed_out", "unavailable")

    fun parse(json: String): StatusPayload {
        val root = try {
            JSONObject(json)
        } catch (error: Exception) {
            throw StatusParseException("响应不是有效的 JSON 对象", error)
        }

        if (root.requiredInt("schemaVersion") != 1) {
            throw StatusParseException("不支持的 schemaVersion")
        }

        val activityJson = root.requiredObject("activity")
        val activity = ActivityStatus(
            executing = activityJson.nonNegativeInt("executing"),
            waitingOnApproval = activityJson.nonNegativeInt("waitingOnApproval"),
            waitingOnUserInput = activityJson.nonNegativeInt("waitingOnUserInput"),
            source = activityJson.enumString("source", setOf("hooks", "unavailable")),
            observedAt = activityJson.requiredInstant("observedAt"),
            stale = activityJson.requiredBoolean("stale"),
        )

        val attemptJson = root.requiredObject("latestAttempt")
        val attempt = AttemptStatus(
            status = attemptJson.enumString("status", allowedStatuses),
            message = attemptJson.optionalNullableString("message"),
            attemptedAt = attemptJson.requiredInstant("attemptedAt"),
        )

        val snapshot = when {
            !root.has("lastGoodSnapshot") -> throw StatusParseException("缺少字段 lastGoodSnapshot")
            root.isNull("lastGoodSnapshot") -> null
            else -> parseSnapshot(root.requiredObject("lastGoodSnapshot"))
        }

        return StatusPayload(
            sourceId = root.nonBlankString("sourceId"),
            revision = root.nonNegativeLong("revision"),
            collectorVersion = root.nonBlankString("collectorVersion"),
            collectedAt = root.requiredInstant("collectedAt"),
            receivedAt = root.optionalInstant("receivedAt"),
            activity = activity,
            latestAttempt = attempt,
            lastGoodSnapshot = snapshot,
        )
    }

    private fun parseSnapshot(json: JSONObject): ProviderSnapshot {
        if (json.nonBlankString("provider") != "codex") {
            throw StatusParseException("lastGoodSnapshot.provider 必须为 codex")
        }

        // 读取但不展示这些字段，以尽早发现与 v1 Schema 不兼容的响应。
        json.nullableNonNegativeInt("resetCredits")
        json.requiredInstantArray("resetCreditExpiresAt")

        return ProviderSnapshot(
            displayName = json.nonBlankString("displayName"),
            plan = json.nullableString("plan"),
            shortWindow = json.nullableWindow("shortWindow"),
            weeklyWindow = json.nullableWindow("weeklyWindow"),
            updatedAt = json.requiredInstant("updatedAt"),
            status = json.enumString("status", allowedStatuses),
            message = json.nullableString("message"),
            nextResetAt = json.nullableInstant("nextResetAt"),
            nextResetWindow = json.nullableEnumString("nextResetWindow", setOf("5h", "weekly")),
        )
    }

    private fun JSONObject.nullableWindow(name: String): UsageWindow? {
        if (!has(name)) throw StatusParseException("缺少字段 $name")
        if (isNull(name)) return null
        val value = requiredObject(name)
        val remaining = value.requiredDouble("remainingPercent")
        if (remaining !in 0.0..100.0) {
            throw StatusParseException("$name.remainingPercent 超出 0..100")
        }
        return UsageWindow(
            remainingPercent = remaining,
            resetsAt = value.nullableInstant("resetsAt"),
            windowSeconds = value.nullableNonNegativeLong("windowSeconds"),
        )
    }

    private fun JSONObject.requiredObject(name: String): JSONObject =
        if (!has(name) || isNull(name)) throw StatusParseException("缺少对象字段 $name")
        else optJSONObject(name) ?: throw StatusParseException("字段 $name 不是对象")

    private fun JSONObject.nonBlankString(name: String): String {
        val value = requiredString(name)
        if (value.isBlank()) throw StatusParseException("字段 $name 不能为空")
        return value
    }

    private fun JSONObject.requiredString(name: String): String {
        if (!has(name) || isNull(name)) throw StatusParseException("缺少字符串字段 $name")
        return try {
            getString(name)
        } catch (error: Exception) {
            throw StatusParseException("字段 $name 不是字符串", error)
        }
    }

    private fun JSONObject.nullableString(name: String): String? {
        if (!has(name)) throw StatusParseException("缺少字段 $name")
        if (isNull(name)) return null
        return requiredString(name)
    }

    private fun JSONObject.optionalNullableString(name: String): String? =
        if (!has(name) || isNull(name)) null else requiredString(name)

    private fun JSONObject.enumString(name: String, allowed: Set<String>): String {
        val value = requiredString(name)
        if (value !in allowed) throw StatusParseException("字段 $name 的值不受支持: $value")
        return value
    }

    private fun JSONObject.nullableEnumString(name: String, allowed: Set<String>): String? {
        val value = nullableString(name) ?: return null
        if (value !in allowed) throw StatusParseException("字段 $name 的值不受支持: $value")
        return value
    }

    private fun JSONObject.requiredInt(name: String): Int = try {
        if (!has(name) || isNull(name)) throw StatusParseException("缺少整数型字段 $name")
        getInt(name)
    } catch (error: StatusParseException) {
        throw error
    } catch (error: Exception) {
        throw StatusParseException("字段 $name 不是整数", error)
    }

    private fun JSONObject.nonNegativeInt(name: String): Int =
        requiredInt(name).also { if (it < 0) throw StatusParseException("字段 $name 不能小于 0") }

    private fun JSONObject.nonNegativeLong(name: String): Long = try {
        if (!has(name) || isNull(name)) throw StatusParseException("缺少整数型字段 $name")
        getLong(name).also { if (it < 0L) throw StatusParseException("字段 $name 不能小于 0") }
    } catch (error: StatusParseException) {
        throw error
    } catch (error: Exception) {
        throw StatusParseException("字段 $name 不是整数", error)
    }

    private fun JSONObject.nullableNonNegativeInt(name: String): Int? {
        if (!has(name)) throw StatusParseException("缺少字段 $name")
        if (isNull(name)) return null
        return nonNegativeInt(name)
    }

    private fun JSONObject.nullableNonNegativeLong(name: String): Long? {
        if (!has(name)) throw StatusParseException("缺少字段 $name")
        if (isNull(name)) return null
        return nonNegativeLong(name)
    }

    private fun JSONObject.requiredDouble(name: String): Double = try {
        if (!has(name) || isNull(name)) throw StatusParseException("缺少数值字段 $name")
        getDouble(name).also { if (!it.isFinite()) throw StatusParseException("字段 $name 不是有限数值") }
    } catch (error: StatusParseException) {
        throw error
    } catch (error: Exception) {
        throw StatusParseException("字段 $name 不是数值", error)
    }

    private fun JSONObject.requiredBoolean(name: String): Boolean = try {
        if (!has(name) || isNull(name)) throw StatusParseException("缺少布尔字段 $name")
        getBoolean(name)
    } catch (error: StatusParseException) {
        throw error
    } catch (error: Exception) {
        throw StatusParseException("字段 $name 不是布尔值", error)
    }

    private fun JSONObject.requiredInstant(name: String): Instant =
        parseInstant(requiredString(name), name)

    private fun JSONObject.optionalInstant(name: String): Instant? =
        if (!has(name) || isNull(name)) null else parseInstant(requiredString(name), name)

    private fun JSONObject.nullableInstant(name: String): Instant? {
        if (!has(name)) throw StatusParseException("缺少字段 $name")
        return if (isNull(name)) null else parseInstant(requiredString(name), name)
    }

    private fun JSONObject.requiredInstantArray(name: String) {
        if (!has(name) || isNull(name)) throw StatusParseException("缺少数组字段 $name")
        val array: JSONArray = optJSONArray(name) ?: throw StatusParseException("字段 $name 不是数组")
        if (array.length() > 64) throw StatusParseException("字段 $name 超过 64 项")
        for (index in 0 until array.length()) {
            parseInstant(array.optString(index, ""), "$name[$index]")
        }
    }

    private fun parseInstant(value: String, name: String): Instant = try {
        Instant.parse(value)
    } catch (error: DateTimeParseException) {
        throw StatusParseException("字段 $name 不是 ISO-8601 时间", error)
    }
}

class StatusParseException(message: String, cause: Throwable? = null) : Exception(message, cause)
