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
import com.mtn888.codexquotasync.ui.ConfigurationActivity

object WidgetUpdater {
    enum class Mode { ONLINE, OFFLINE, REFRESHING, CACHED, NOT_CONFIGURED }

    fun showInitial(context: Context) {
        val repository = StatusRepository(context)
        if (repository.configuredBaseUrl() == null) {
            updateAll(context, null, Mode.NOT_CONFIGURED)
        } else {
            updateAll(context, repository.loadCached()?.payload, Mode.CACHED)
        }
    }

    fun showRefreshing(context: Context) {
        val cached = StatusRepository(context).loadCached()?.payload
        updateAll(context, cached, Mode.REFRESHING)
    }

    fun showRefreshing(context: Context, appWidgetId: Int) {
        val cached = StatusRepository(context).loadCached()?.payload
        updateOne(context, appWidgetId, cached, Mode.REFRESHING)
    }

    fun showOnline(context: Context, payload: StatusPayload) =
        updateAll(context, payload, Mode.ONLINE)

    fun showOffline(context: Context, message: String?) {
        val cached = StatusRepository(context).loadCached()?.payload
        updateAll(context, cached, Mode.OFFLINE, message)
    }

    private fun updateAll(
        context: Context,
        payload: StatusPayload?,
        mode: Mode,
        message: String? = null,
    ) {
        val manager = AppWidgetManager.getInstance(context)
        val component = ComponentName(context, QuotaWidgetProvider::class.java)
        manager.getAppWidgetIds(component).forEach { id ->
            manager.updateAppWidget(id, buildViews(context, id, payload, mode, message))
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
        AppWidgetManager.getInstance(context).updateAppWidget(
            appWidgetId,
            buildViews(context, appWidgetId, payload, mode, message),
        )
    }

    private fun buildViews(
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
            payload?.activity?.let(StatusFormatter::activity) ?: "执行中 —   待审批 —   待输入 —",
        )

        val display = displayStatus(context, payload, mode, message)
        views.setTextViewText(R.id.text_connection_status, display.label)
        views.setTextColor(R.id.text_connection_status, display.color)
        views.setTextViewText(R.id.text_last_updated, display.detail)

        views.setOnClickPendingIntent(R.id.button_refresh, refreshIntent(context, appWidgetId))
        views.setOnClickPendingIntent(R.id.widget_root, configurationIntent(context, appWidgetId))
        views.setViewVisibility(R.id.button_refresh, View.VISIBLE)
        return views
    }

    private fun displayStatus(
        context: Context,
        payload: StatusPayload?,
        mode: Mode,
        message: String?,
    ): DisplayStatus {
        val updateTime = payload?.let(StatusFormatter::updateTime)
        return when (mode) {
            Mode.NOT_CONFIGURED -> DisplayStatus(
                label = "未配置",
                detail = "点击小组件填写服务器地址",
                color = context.getColor(R.color.widget_warning),
            )
            Mode.REFRESHING -> DisplayStatus(
                label = "刷新中",
                detail = updateTime?.let { "正在刷新 · 上次更新 $it" } ?: "正在从服务器获取状态…",
                color = context.getColor(R.color.widget_warning),
            )
            Mode.OFFLINE -> DisplayStatus(
                label = "离线",
                detail = buildString {
                    append(updateTime?.let { "缓存更新 $it" } ?: "没有可用缓存")
                    message?.takeIf { it.isNotBlank() }?.let { append(" · ${it.take(80)}") }
                },
                color = context.getColor(R.color.widget_error),
            )
            Mode.CACHED -> DisplayStatus(
                label = "缓存",
                detail = updateTime?.let { "缓存更新 $it · 正在等待刷新" } ?: "正在等待首次同步",
                color = context.getColor(R.color.widget_warning),
            )
            Mode.ONLINE -> {
                val stale = payload == null || StatusFormatter.isStale(payload)
                DisplayStatus(
                    label = if (stale) "过期" else "在线",
                    detail = updateTime?.let {
                        if (stale) "数据已过期 · 更新 $it" else "更新 $it"
                    } ?: "服务器没有可用额度快照",
                    color = context.getColor(if (stale) R.color.widget_warning else R.color.widget_accent),
                )
            }
        }
    }

    private fun refreshIntent(context: Context, appWidgetId: Int): PendingIntent {
        val intent = Intent(context, QuotaWidgetProvider::class.java).apply {
            action = QuotaWidgetProvider.ACTION_REFRESH
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

    private fun configurationIntent(context: Context, appWidgetId: Int): PendingIntent {
        val intent = Intent(context, ConfigurationActivity::class.java).apply {
            putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, appWidgetId)
            data = Uri.parse("codexquotasync://config/$appWidgetId")
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
