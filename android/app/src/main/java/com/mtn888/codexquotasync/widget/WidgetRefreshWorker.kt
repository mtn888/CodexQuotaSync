package com.mtn888.codexquotasync.widget

import android.content.Context
import androidx.work.Worker
import androidx.work.WorkerParameters
import com.mtn888.codexquotasync.data.ConfigurationException
import com.mtn888.codexquotasync.data.StatusRepository

class WidgetRefreshWorker(
    appContext: Context,
    workerParameters: WorkerParameters,
) : Worker(appContext, workerParameters) {

    override fun doWork(): Result {
        val repository = StatusRepository(applicationContext)
        return try {
            val payload = repository.fetch()
            WidgetUpdater.showOnline(applicationContext, payload)
            Result.success()
        } catch (error: ConfigurationException) {
            WidgetUpdater.showInitial(applicationContext)
            Result.failure()
        } catch (error: Exception) {
            WidgetUpdater.showOffline(applicationContext, error.message ?: "同步失败")
            // Network and transient server failures must be retried. Treating
            // them as success made an OEM-delayed periodic run wait another
            // full scheduling window and left the widget offline indefinitely.
            Result.retry()
        }
    }
}
