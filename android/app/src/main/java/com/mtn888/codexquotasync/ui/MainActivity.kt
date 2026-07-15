package com.mtn888.codexquotasync.ui

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.view.View
import android.widget.Button
import android.widget.TextView
import android.widget.Toast
import com.mtn888.codexquotasync.R
import com.mtn888.codexquotasync.data.StatusFormatter
import com.mtn888.codexquotasync.data.StatusPayload
import com.mtn888.codexquotasync.data.StatusRepository
import com.mtn888.codexquotasync.widget.WidgetScheduler
import com.mtn888.codexquotasync.widget.WidgetUpdater
import java.util.Locale

class MainActivity : Activity() {
    private lateinit var repository: StatusRepository
    private lateinit var refreshButton: Button
    private lateinit var progress: View

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        repository = StatusRepository(this)
        refreshButton = findViewById(R.id.button_detail_refresh)
        progress = findViewById(R.id.refresh_progress)
        refreshButton.setOnClickListener { refreshNow() }
        findViewById<Button>(R.id.button_open_settings).setOnClickListener {
            startActivity(Intent(this, ConfigurationActivity::class.java))
        }
        WidgetScheduler.schedulePeriodic(this)
        renderCached()
    }

    override fun onResume() {
        super.onResume()
        renderCached()
    }

    private fun renderCached() {
        val cached = repository.loadCached()
        render(cached?.payload)
        if (cached == null && repository.configuredBaseUrl() != null) {
            WidgetScheduler.enqueueImmediate(this)
        }
    }

    private fun refreshNow() {
        if (repository.configuredBaseUrl() == null) {
            startActivity(Intent(this, ConfigurationActivity::class.java))
            return
        }
        setRefreshing(true)
        Thread {
            try {
                val payload = repository.fetch()
                WidgetUpdater.showOnline(applicationContext, payload)
                runOnUiThread {
                    render(payload)
                    setRefreshing(false)
                }
            } catch (error: Exception) {
                WidgetUpdater.showOffline(applicationContext, error.message)
                runOnUiThread {
                    render(repository.loadCached()?.payload, error.message)
                    setRefreshing(false)
                    Toast.makeText(this, error.message ?: "刷新失败", Toast.LENGTH_LONG).show()
                }
            }
        }.start()
    }

    private fun setRefreshing(refreshing: Boolean) {
        refreshButton.isEnabled = !refreshing
        refreshButton.text = if (refreshing) "刷新中…" else "立即刷新"
        progress.visibility = if (refreshing) View.VISIBLE else View.GONE
    }

    private fun render(payload: StatusPayload?, error: String? = null) {
        val snapshot = payload?.lastGoodSnapshot
        val configured = repository.configuredBaseUrl() != null
        val stale = payload?.let(StatusFormatter::isStale) ?: true
        text(R.id.text_detail_connection).text = when {
            !configured -> "尚未配置服务器"
            error != null -> "网络断联 · ${error.take(100)}"
            payload == null -> "等待首次后台同步"
            stale -> "已连接 · 数据可能过期"
            else -> "已连接 · 数据最新"
        }
        text(R.id.text_detail_source).text = payload?.let {
            "${it.sourceId} · Collector ${it.collectorVersion}"
        } ?: repository.configuredBaseUrl().orEmpty()
        text(R.id.text_detail_short_quota).text = StatusFormatter.percentage(snapshot?.shortWindow)
        text(R.id.text_detail_weekly_quota).text = StatusFormatter.percentage(snapshot?.weeklyWindow)
        text(R.id.text_detail_reset).text = StatusFormatter.nextReset(snapshot)
        text(R.id.text_detail_executing).text =
            String.format(Locale.getDefault(), "%d", payload?.activity?.executing ?: 0)
        text(R.id.text_detail_pending).text =
            String.format(
                Locale.getDefault(),
                "%d",
                payload?.activity?.let(StatusFormatter::pending) ?: 0,
            )
        text(R.id.text_detail_updated).text = payload?.let {
            "服务器采集：${StatusFormatter.updateTime(it)}\n最近结果：${it.latestAttempt.status}${it.latestAttempt.message?.let { message -> " · $message" }.orEmpty()}"
        } ?: "尚无缓存数据"
    }

    private fun text(id: Int): TextView = findViewById(id)
}
