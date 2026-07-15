package com.mtn888.codexquotasync.widget

import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.content.Intent

open class BaseQuotaWidgetProvider : AppWidgetProvider() {
    override fun onEnabled(context: Context) {
        super.onEnabled(context)
        WidgetScheduler.schedulePeriodic(context)
        WidgetUpdater.showInitial(context)
        WidgetScheduler.enqueueImmediate(context)
    }

    override fun onUpdate(context: Context, manager: AppWidgetManager, appWidgetIds: IntArray) {
        super.onUpdate(context, manager, appWidgetIds)
        WidgetScheduler.schedulePeriodic(context)
        WidgetUpdater.showInitial(context)
        WidgetScheduler.enqueueImmediate(context)
    }

    override fun onReceive(context: Context, intent: Intent) {
        super.onReceive(context, intent)
        if (intent.action == ACTION_REFRESH) {
            WidgetUpdater.showRefreshing(context)
            WidgetScheduler.enqueueImmediate(context)
        }
    }

    override fun onDisabled(context: Context) {
        if (!WidgetUpdater.hasAnyWidgets(context)) {
            WidgetScheduler.cancelPeriodic(context)
        }
        super.onDisabled(context)
    }

    companion object {
        const val ACTION_REFRESH = "com.mtn888.codexquotasync.action.REFRESH"
    }
}

class QuotaWidgetProvider : BaseQuotaWidgetProvider()

class QuotaCompactWidgetProvider : BaseQuotaWidgetProvider()

class QuotaWideWidgetProvider : BaseQuotaWidgetProvider()
