package com.mtn888.codexquotasync.ui

import android.app.Activity
import android.appwidget.AppWidgetManager
import android.content.Intent
import android.os.Bundle
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.TextView
import android.widget.Toast
import com.mtn888.codexquotasync.R
import com.mtn888.codexquotasync.data.ConfigurationException
import com.mtn888.codexquotasync.data.StatusRepository
import com.mtn888.codexquotasync.widget.WidgetScheduler
import com.mtn888.codexquotasync.widget.WidgetUpdater

class ConfigurationActivity : Activity() {
    private var appWidgetId: Int = AppWidgetManager.INVALID_APPWIDGET_ID

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_configuration)

        appWidgetId = intent?.getIntExtra(
            AppWidgetManager.EXTRA_APPWIDGET_ID,
            AppWidgetManager.INVALID_APPWIDGET_ID,
        ) ?: AppWidgetManager.INVALID_APPWIDGET_ID
        if (appWidgetId != AppWidgetManager.INVALID_APPWIDGET_ID) {
            setResult(RESULT_CANCELED)
        }

        val repository = StatusRepository(this)
        val input = findViewById<EditText>(R.id.input_base_url)
        val validation = findViewById<TextView>(R.id.text_validation_error)
        input.setText(repository.configuredBaseUrl().orEmpty())

        findViewById<Button>(R.id.button_save).setOnClickListener {
            try {
                repository.saveBaseUrl(input.text.toString())
                validation.visibility = View.GONE
                WidgetScheduler.schedulePeriodic(this)
                if (appWidgetId != AppWidgetManager.INVALID_APPWIDGET_ID) {
                    WidgetUpdater.showRefreshing(this, appWidgetId)
                } else {
                    WidgetUpdater.showRefreshing(this)
                }
                WidgetScheduler.enqueueImmediate(this)
                Toast.makeText(this, R.string.config_saved, Toast.LENGTH_SHORT).show()
                finishSuccessfully()
            } catch (error: ConfigurationException) {
                validation.text = error.message
                validation.visibility = View.VISIBLE
                input.requestFocus()
            }
        }
    }

    private fun finishSuccessfully() {
        if (appWidgetId != AppWidgetManager.INVALID_APPWIDGET_ID) {
            val result = Intent().putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, appWidgetId)
            setResult(RESULT_OK, result)
        }
        finish()
    }
}
