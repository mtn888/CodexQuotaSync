package com.mtn888.codexquotasync.widget

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.view.View
import android.widget.RemoteViews
import com.mtn888.codexquotasync.R
import com.mtn888.codexquotasync.data.StatusFormatter
import com.mtn888.codexquotasync.data.StatusPayload
import com.mtn888.codexquotasync.data.StatusRepository
import com.mtn888.codexquotasync.ui.MainActivity

object WidgetUpdater {
    enum class Mode { ONLINE, OFFLINE, REFRESHING, CACHED, NOT_CONFIGURED }

    private enum class Variant { COMPACT, WIDE, LARGE }

    private val providers = listOf(
        QuotaCompactWidgetProvider::class.java to Variant.COMPACT,
        QuotaWideWidgetProvider::class.java to Variant.WIDE,
        QuotaWidgetProvider::class.java to Variant.LARGE,
    )

    fun showInitial(context: Context) {
        val repository = StatusRepository(context)
        if (repository.configuredBaseUrl() == null) {
            updateAll(context, null, Mode.NOT_CONFIGURED)
        } else {
            updateAll(context, repository.loadCached()?.payload, Mode.CACHED)
        }
    }

    fun showRefreshing(context: Context) {
        updateAll(context, StatusRepository(context).loadCached()?.payload, Mode.REFRESHING)
    }

    fun showRefreshing(context: Context, appWidgetId: Int) {
        updateOne(
            context,
            appWidgetId,
            StatusRepository(context).loadCached()?.payload,
            Mode.REFRESHING,
        )
    }

    fun showOnline(context: Context, payload: StatusPayload) =
        updateAll(context, payload, Mode.ONLINE)

    fun showOffline(context: Context, message: String?) {
        updateAll(
            context,
            StatusRepository(context).loadCached()?.payload,
            Mode.OFFLINE,
            message,
        )
    }

    fun hasAnyWidgets(context: Context): Boolean {
        val manager = AppWidgetManager.getInstance(context)
        return providers.any { (provider, _) ->
            manager.getAppWidgetIds(ComponentName(context, provider)).isNotEmpty()
        }
    }

    private fun updateAll(
        context: Context,
        payload: StatusPayload?,
        mode: Mode,
        message: String? = null,
    ) {
        val manager = AppWidgetManager.getInstance(context)
        providers.forEach { (provider, variant) ->
            manager.getAppWidgetIds(ComponentName(context, provider)).forEach { id ->
                manager.updateAppWidget(id, buildViews(context, id, variant, payload, mode, message))
            }
        }
    }

    private fun updateOne(
        context: Context,
        appWidgetId: Int,
        payload: StatusPayload?,
        mode: Mode,
        message: String? = null,
    ) {
        if (appWidgetId == AppWidgetManager.INVALID_APPWIDGET_ID) return
        val manager = AppWidgetManager.getInstance(context)
        val providerName = manager.getAppWidgetInfo(appWidgetId)?.provider?.className
        val variant = providers.firstOrNull { (provider, _) -> provider.name == providerName }?.second
            ?: Variant.LARGE
        manager.updateAppWidget(
            appWidgetId,
            buildViews(context, appWidgetId, variant, payload, mode, message),
        )
    }

    private fun buildViews(
        context: Context,
        appWidgetId: Int,
        variant: Variant,
        payload: StatusPayload?,
        mode: Mode,
        message: String?,
    ): RemoteViews = when (variant) {
        Variant.COMPACT -> buildCompactViews(context, appWidgetId, payload)
        Variant.WIDE -> buildWideViews(context, appWidgetId, payload)
        Variant.LARGE -> buildLargeViews(context, appWidgetId, payload, mode, message)
    }

    private fun buildCompactViews(
        context: Context,
        appWidgetId: Int,
        payload: StatusPayload?,
    ): RemoteViews {
        val views = RemoteViews(context.packageName, R.layout.widget_quota_compact)
        val snapshot = payload?.lastGoodSnapshot
        val primary = snapshot?.shortWindow ?: snapshot?.weeklyWindow
        val pending = payload?.activity?.let(StatusFormatter::pending) ?: 0
        val executing = payload?.activity?.executing ?: 0
        views.setTextViewText(R.id.text_compact_quota, StatusFormatter.percentage(primary))
        views.setTextViewText(R.id.text_compact_window, if (snapshot?.shortWindow != null) "5H" else "W")
        views.setTextViewText(R.id.compact_pending_badge, pending.toString())
        views.setViewVisibility(R.id.compact_pending_badge, if (pending > 0) View.VISIBLE else View.GONE)
        views.setViewVisibility(R.id.compact_running_dot, if (executing > 0) View.VISIBLE else View.GONE)
        applyRoot(context, views, R.id.widget_root_compact, appWidgetId, payload, paddingDp = 6)
        return views
    }

    private fun buildWideViews(
        context: Context,
        appWidgetId: Int,
        payload: StatusPayload?,
    ): RemoteViews {
        val views = RemoteViews(context.packageName, R.layout.widget_quota_wide)
        val snapshot = payload?.lastGoodSnapshot
        val primary = snapshot?.shortWindow ?: snapshot?.weeklyWindow
        views.setTextViewText(R.id.text_wide_quota, StatusFormatter.percentage(primary))
        views.setTextViewText(R.id.text_wide_executing, (payload?.activity?.executing ?: 0).toString())
        views.setTextViewText(
            R.id.text_wide_pending,
            (payload?.activity?.let(StatusFormatter::pending) ?: 0).toString(),
        )
        views.setOnClickPendingIntent(R.id.button_refresh, refreshIntent(context, appWidgetId))
        applyRoot(context, views, R.id.widget_root_wide, appWidgetId, payload, paddingDp = 6)
        return views
    }

    private fun buildLargeViews(
        context: Context,
        appWidgetId: Int,
        payload: StatusPayload?,
        mode: Mode,
        message: String?,
    ): RemoteViews {
        val views = RemoteViews(context.packageName, R.layout.widget_quota)
        val snapshot = payload?.lastGoodSnapshot
        views.setTextViewText(R.id.text_short_quota, StatusFormatter.percentage(snapshot?.shortWindow))
        views.setProgressBar(
            R.id.progress_short_quota,
            100,
            StatusFormatter.progress(snapshot?.shortWindow),
            false,
        )
        views.setTextViewText(R.id.text_weekly_quota, StatusFormatter.percentage(snapshot?.weeklyWindow))
        views.setProgressBar(
            R.id.progress_weekly_quota,
            100,
            StatusFormatter.progress(snapshot?.weeklyWindow),
            false,
        )
        views.setTextViewText(R.id.text_next_reset, StatusFormatter.nextReset(snapshot))
        views.setTextViewText(
            R.id.text_activity,
            payload?.activity?.let(StatusFormatter::activity) ?: "执行中 —   待处理 —",
        )
        val display = displayStatus(context, payload, mode, message)
        views.setTextViewText(R.id.text_connection_status, display.label)
        views.setTextColor(R.id.text_connection_status, display.color)
        views.setTextViewText(R.id.text_last_updated, display.detail)
        views.setOnClickPendingIntent(R.id.button_refresh, refreshIntent(context, appWidgetId))
        applyRoot(context, views, R.id.widget_root, appWidgetId, payload, paddingDp = 12)
        return views
    }

    private fun applyRoot(
        context: Context,
        views: RemoteViews,
        rootId: Int,
        appWidgetId: Int,
        payload: StatusPayload?,
        paddingDp: Int,
    ) {
        views.setInt(rootId, "setBackgroundResource", backgroundFor(payload))
        val padding = (paddingDp * context.resources.displayMetrics.density).toInt()
        views.setViewPadding(rootId, padding, padding, padding, padding)
        views.setOnClickPendingIntent(rootId, statusIntent(context, appWidgetId))
    }

    private fun backgroundFor(payload: StatusPayload?): Int {
        val activity = payload?.activity ?: return R.drawable.widget_background_idle
        return when {
            StatusFormatter.pending(activity) > 0 -> R.drawable.widget_background_pending
            activity.executing > 0 -> R.drawable.widget_background_running
            else -> R.drawable.widget_background_idle
        }
    }

    private fun displayStatus(
        context: Context,
        payload: StatusPayload?,
        mode: Mode,
        message: String?,
    ): DisplayStatus {
        val updateTime = payload?.let(StatusFormatter::updateTime)
        return when (mode) {
            Mode.NOT_CONFIGURED -> DisplayStatus("未配置", "打开应用填写服务器地址", context.getColor(R.color.widget_warning))
            Mode.REFRESHING -> DisplayStatus(
                "刷新中",
                updateTime?.let { "正在刷新 · 上次更新 $it" } ?: "正在从服务器获取状态…",
                context.getColor(R.color.widget_warning),
            )
            Mode.OFFLINE -> DisplayStatus(
                "离线",
                buildString {
                    append(updateTime?.let { "缓存更新 $it" } ?: "没有可用缓存")
                    message?.takeIf { it.isNotBlank() }?.let { append(" · ${it.take(80)}") }
                },
                context.getColor(R.color.widget_error),
            )
            Mode.CACHED -> DisplayStatus(
                "缓存",
                updateTime?.let { "缓存更新 $it · 正在等待刷新" } ?: "正在等待首次同步",
                context.getColor(R.color.widget_warning),
            )
            Mode.ONLINE -> {
                val stale = payload == null || StatusFormatter.isStale(payload)
                DisplayStatus(
                    if (stale) "过期" else "在线",
                    updateTime?.let { if (stale) "数据已过期 · 更新 $it" else "更新 $it" }
                        ?: "服务器没有可用额度快照",
                    context.getColor(if (stale) R.color.widget_warning else R.color.widget_accent),
                )
            }
        }
    }

    private fun refreshIntent(context: Context, appWidgetId: Int): PendingIntent {
        val intent = Intent(context, QuotaWidgetProvider::class.java).apply {
            action = BaseQuotaWidgetProvider.ACTION_REFRESH
            putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, appWidgetId)
            data = Uri.parse("codexquotasync://refresh/$appWidgetId")
        }
        return PendingIntent.getBroadcast(
            context,
            appWidgetId,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }

    private fun statusIntent(context: Context, appWidgetId: Int): PendingIntent {
        val intent = Intent(context, MainActivity::class.java).apply {
            data = Uri.parse("codexquotasync://status/$appWidgetId")
        }
        return PendingIntent.getActivity(
            context,
            100_000 + appWidgetId,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }

    private data class DisplayStatus(val label: String, val detail: String, val color: Int)
}
