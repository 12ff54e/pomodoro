package com.pomodoro.app

import android.os.Bundle
import android.util.Log
import android.webkit.WebView
import android.webkit.WebViewClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import androidx.webkit.WebViewAssetLoader

class MainActivity : TauriActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        Log.i("Pomodoro", "onCreate START")
        WebView.setWebContentsDebuggingEnabled(true)
        super.onCreate(savedInstanceState)
        Log.i("Pomodoro", "onCreate DONE")
    }

    override fun onWebViewCreate(webView: WebView) {
        Log.i("Pomodoro", "onWebViewCreate — setting up asset loader for tauri.localhost")

        // Set up our own asset loader because Tauri doesn't enable
        // with_asset_loader on Android. This serves files from the
        // APK assets directory when the WebView requests
        // https://tauri.localhost/*
        val assetLoader = WebViewAssetLoader.Builder()
            .setDomain("tauri.localhost")
            .addPathHandler("/", WebViewAssetLoader.AssetsPathHandler(this))
            .build()

        // Wrap the existing WebViewClient to intercept tauri.localhost
        val originalClient = webView.webViewClient
        webView.webViewClient = object : WebViewClient() {
            override fun shouldInterceptRequest(
                view: WebView,
                request: WebResourceRequest
            ): WebResourceResponse? {
                val url = request.url.toString()
                if (url.contains("tauri.localhost")) {
                    val response = assetLoader.shouldInterceptRequest(request.url)
                    if (response != null) {
                        Log.d("Pomodoro", "AssetLoader served: $url")
                        return response
                    }
                }
                // Fall back to the original client for everything else
                return originalClient.shouldInterceptRequest(view, request)
            }
        }

        // Now load the frontend through the asset loader
        webView.loadUrl("https://tauri.localhost/index.html")
        Log.i("Pomodoro", "Loading https://tauri.localhost/index.html")
    }
}
