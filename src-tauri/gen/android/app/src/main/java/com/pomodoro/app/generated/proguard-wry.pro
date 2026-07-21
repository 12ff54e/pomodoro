# THIS FILE IS AUTO-GENERATED. DO NOT MODIFY!!

# Copyright 2020-2023 Tauri Programme within The Commons Conservancy
# SPDX-License-Identifier: Apache-2.0
# SPDX-License-Identifier: MIT

-keep class com.pomodoro.app.* {
  native <methods>;
}

-keep class com.pomodoro.app.WryActivity {
  public <init>(...);

  void setWebView(com.pomodoro.app.RustWebView);
  java.lang.Class getAppClass(...);
  int getId();
  java.lang.String getVersion();
  int startActivity(...);
}

-keep class com.pomodoro.app.Ipc {
  public <init>(...);

  @android.webkit.JavascriptInterface public <methods>;
}

-keep class com.pomodoro.app.RustWebView {
  public <init>(...);

  void loadUrlMainThread(...);
  void loadHTMLMainThread(...);
  void evalScript(...);
}

-keep class com.pomodoro.app.RustWebChromeClient,com.pomodoro.app.RustWebViewClient {
  public <init>(...);
}
