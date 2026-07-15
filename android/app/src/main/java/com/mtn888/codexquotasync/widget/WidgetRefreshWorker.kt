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
            // 周期任务会在下一轮（15 分钟）再次尝试；手动刷新失败后由用户决定是否重试。
            // 返回 success 可避免网络故障时触发 WorkManager 默认的 10 分钟退避重试，
            // 从而维持约定的低频轮询。
            Result.success()
        }
    }
}
