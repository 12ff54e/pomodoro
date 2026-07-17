// Minimal W3C WebDriver HTTP client. Wraps tauri-driver's WebDriver
// JSON wire protocol using only `ureq` and `serde_json` — no async,
// no tokio, no npm. Covers exactly the operations we need for E2E tests.

use serde_json::Value;

/// A handle to a WebDriver session for a running Tauri app.
pub struct WebDriverClient {
    base_url: String,
    session_id: String,
}

impl WebDriverClient {
    /// Create a new WebDriver session for the given Tauri app binary.
    ///
    /// `driver_url` — e.g. `"http://127.0.0.1:4445"`
    /// `app_path`  — absolute path to the Tauri binary, e.g.
    ///               `"C:\\Users\\qzhong\\pomodoro\\src-tauri\\target\\debug\\pomodoro.exe"`
    pub fn new_session(driver_url: &str, app_path: &str) -> Result<Self, String> {
        let create_url = format!("{}/session", driver_url);

        let body = serde_json::json!({
            "capabilities": {
                "alwaysMatch": {
                    "browserName": "tauri",
                    "tauri:options": {
                        "application": app_path
                    },
                    "ms:edgeOptions": {
                        "args": ["--headless=new"]
                    }
                }
            }
        });

        let resp: Value = match ureq::post(&create_url)
            .set("Content-Type", "application/json")
            .send_json(&body)
        {
            Ok(r) => r.into_json().map_err(|e| format!("POST /session parse error: {}", e))?,
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                return Err(format!(
                    "POST /session returned HTTP {}: {}",
                    code,
                    &body[..body.len().min(500)]
                ));
            }
            Err(e) => return Err(format!("POST /session transport error: {}", e)),
        };

        let value = &resp["value"];

        // Check for WebDriver error in the response.
        if let Some(err) = value.get("error").and_then(|v| v.as_str()) {
            let msg = value
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("(no message)");
            return Err(format!("WebDriver error: {} — {}", err, msg));
        }

        let session_id = value["sessionId"]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "No sessionId in response: {}",
                    serde_json::to_string_pretty(&resp).unwrap_or_default()
                )
            })?
            .to_string();

        Ok(Self {
            base_url: driver_url.to_string(),
            session_id,
        })
    }

    /// DELETE /session/:id — close the session and shut down the app.
    pub fn delete_session(self) -> Result<(), String> {
        let url = format!("{}/session/{}", self.base_url, self.session_id);
        ureq::delete(&url)
            .call()
            .map_err(|e| format!("DELETE /session failed: {}", e))?;
        Ok(())
    }

    // ---- Element finding ----

    /// Find a single element by CSS selector.
    pub fn find_element(&self, css: &str) -> Result<String, String> {
        let url = format!("{}/session/{}/element", self.base_url, self.session_id);
        let body = serde_json::json!({
            "using": "css selector",
            "value": css
        });

        let resp: Value = ureq::post(&url)
            .set("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| format!("find_element '{}' failed: {}", css, e))?
            .into_json()
            .map_err(|e| format!("find_element '{}' parse error: {}", css, e))?;

        let element_id = extract_element_id(&resp, css)?;
        Ok(element_id.to_string())
    }

    /// Find all elements matching a CSS selector.
    pub fn find_elements(&self, css: &str) -> Result<Vec<String>, String> {
        let url = format!("{}/session/{}/elements", self.base_url, self.session_id);
        let body = serde_json::json!({
            "using": "css selector",
            "value": css
        });

        let resp: Value = ureq::post(&url)
            .set("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| format!("find_elements '{}' failed: {}", css, e))?
            .into_json()
            .map_err(|e| format!("find_elements '{}' parse error: {}", css, e))?;

        let arr = resp["value"]
            .as_array()
            .ok_or_else(|| format!("find_elements '{}': value is not an array", css))?;

        let mut ids = Vec::new();
        for entry in arr {
            if let Some(id) = entry.get("ELEMENT").or_else(|| entry.get("element-6066-11e4-a52e-4f735466cecf")).and_then(|v| v.as_str()) {
                ids.push(id.to_string());
            }
        }
        Ok(ids)
    }

    // ---- Element interaction ----

    /// Read the text content of an element.
    pub fn element_text(&self, element_id: &str) -> Result<String, String> {
        let url = format!(
            "{}/session/{}/element/{}/text",
            self.base_url, self.session_id, element_id
        );
        let resp: Value = ureq::get(&url)
            .call()
            .map_err(|e| format!("element_text failed: {}", e))?
            .into_json()
            .map_err(|e| format!("element_text parse error: {}", e))?;

        resp["value"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "element_text: value is not a string".to_string())
    }

    /// Click an element.
    pub fn element_click(&self, element_id: &str) -> Result<(), String> {
        let url = format!(
            "{}/session/{}/element/{}/click",
            self.base_url, self.session_id, element_id
        );
        let body = serde_json::json!({});
        ureq::post(&url)
            .set("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| format!("element_click failed: {}", e))?;
        Ok(())
    }

    /// Read a DOM property of an element (e.g. "className", "textContent").
    pub fn element_property(&self, element_id: &str, name: &str) -> Result<String, String> {
        let url = format!(
            "{}/session/{}/element/{}/property/{}",
            self.base_url, self.session_id, element_id, name
        );
        let resp: Value = ureq::get(&url)
            .call()
            .map_err(|e| format!("element_property '{}' failed: {}", name, e))?
            .into_json()
            .map_err(|e| format!("element_property '{}' parse error: {}", name, e))?;

        // Property values can be any JSON type — convert to string.
        let v = &resp["value"];
        match v {
            Value::String(s) => Ok(s.clone()),
            Value::Bool(b) => Ok(b.to_string()),
            Value::Number(n) => Ok(n.to_string()),
            Value::Null => Ok("null".to_string()),
            _ => Ok(v.to_string()),
        }
    }

    /// Execute JavaScript in the WebView and return the result.
    pub fn execute_script(&self, script: &str, args: &[Value]) -> Result<Value, String> {
        let url = format!(
            "{}/session/{}/execute/sync",
            self.base_url, self.session_id
        );
        let body = serde_json::json!({
            "script": script,
            "args": args
        });

        let resp: Value = ureq::post(&url)
            .set("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| format!("execute_script failed: {}", e))?
            .into_json()
            .map_err(|e| format!("execute_script parse error: {}", e))?;

        Ok(resp["value"].clone())
    }

    // ---- Wait helpers ----

    /// Poll until an element matching `css` exists, or timeout.
    #[allow(dead_code)]
    pub fn wait_for_element(&self, css: &str, timeout_secs: u64) -> Result<String, String> {
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_secs(timeout_secs);
        loop {
            match self.find_element(css) {
                Ok(id) => return Ok(id),
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Poll until an element's text is non-empty, or timeout.
    #[allow(dead_code)]
    pub fn wait_for_text(&self, css: &str, timeout_secs: u64) -> Result<String, String> {
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_secs(timeout_secs);
        loop {
            match self.find_element(css) {
                Ok(id) => match self.element_text(&id) {
                    Ok(text) if !text.is_empty() => return Ok(text),
                    _ if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                    Err(e) => return Err(e),
                    _ => return Err(format!("wait_for_text '{}' timed out", css)),
                },
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Extract an element ID from a WebDriver response (handles both
/// legacy JSON Wire Protocol format and W3C format).
fn extract_element_id<'a>(resp: &'a Value, context: &str) -> Result<&'a str, String> {
    let value = &resp["value"];

    // W3C format: {"element-6066-11e4-a52e-4f735466cecf": "..."}
    if let Some(id) = value
        .get("element-6066-11e4-a52e-4f735466cecf")
        .and_then(|v| v.as_str())
    {
        return Ok(id);
    }

    // Legacy JSON Wire Protocol format: {"ELEMENT": "..."}
    if let Some(id) = value.get("ELEMENT").and_then(|v| v.as_str()) {
        return Ok(id);
    }

    // Some drivers return the value as a plain string.
    if let Some(id) = value.as_str() {
        return Ok(id);
    }

    Err(format!(
        "find_element '{}': cannot extract element ID from {}",
        context,
        serde_json::to_string_pretty(resp).unwrap_or_default()
    ))
}

/// Build the absolute path to the app binary from environment or defaults.
pub fn app_path() -> String {
    if let Ok(p) = std::env::var("APP_PATH") {
        return p;
    }
    // Fallback: assume debug build in standard location.
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().unwrap_or_else(|| std::path::Path::new("."));
    dir.join("pomodoro.exe").to_string_lossy().to_string()
}

/// Build the tauri-driver URL from environment or default.
pub fn driver_url() -> String {
    std::env::var("TAURI_DRIVER_URL").unwrap_or_else(|_| "http://127.0.0.1:4444".to_string())
}
