use std::time::Instant;
use tokio::process::{Child, Command};

/// How long the engine can sit idle before we shut it down.
const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// How long to wait for the engine to become healthy after spawning.
const STARTUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Interval between health-check polls during startup.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

pub struct LTEngine {
    child: Option<Child>,
    last_used: Instant,
    port: u16,
    model: String,
    binary_path: String,
}

impl LTEngine {
    pub fn new(port: u16, model: String, binary_path: String) -> Self {
        Self {
            child: None,
            last_used: Instant::now(),
            port,
            model,
            binary_path,
        }
    }

    /// Returns the base URL for the running engine.
    pub fn base_url(&self) -> String {
        format!("http://localhost:{}", self.port)
    }

    /// Returns true if the child process is still alive.
    fn is_running(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            // try_wait returns Ok(Some(_)) if exited, Ok(None) if still running
            matches!(child.try_wait(), Ok(None))
        } else {
            false
        }
    }

    /// Ensure the engine is running and healthy. Starts it if necessary.
    pub async fn ensure_running(&mut self) -> anyhow::Result<()> {
        self.last_used = Instant::now();

        if self.is_running() {
            // Already running – quick health check
            if self.health_check().await {
                return Ok(());
            }
            // Process alive but not yet responding – wait for it to become ready
            return self.wait_for_healthy().await;
        }

        self.start().await
    }

    /// Spawn the ltengine process.
    async fn start(&mut self) -> anyhow::Result<()> {
        let child = Command::new(&self.binary_path)
            .args(["-m", &self.model, "-p", &self.port.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to start ltengine at '{}' (is it installed and in PATH?): {}",
                    self.binary_path,
                    e
                )
            })?;

        self.child = Some(child);
        self.last_used = Instant::now();

        self.wait_for_healthy().await
    }

    /// Poll until the server responds to health checks or times out.
    async fn wait_for_healthy(&mut self) -> anyhow::Result<()> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        while Instant::now() < deadline {
            if !self.is_running() {
                return Err(anyhow::anyhow!(
                    "ltengine exited unexpectedly – check that the model '{}' is valid",
                    self.model
                ));
            }
            if self.health_check().await {
                return Ok(());
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }

        self.shutdown().await;
        Err(anyhow::anyhow!(
            "ltengine did not become healthy within {} seconds",
            STARTUP_TIMEOUT.as_secs()
        ))
    }

    /// Quick HTTP ping to see if the server is responding.
    async fn health_check(&self) -> bool {
        let url = format!("{}/languages", self.base_url());
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build();
        match client {
            Ok(c) => c
                .get(&url)
                .send()
                .await
                .map_or(false, |r| r.status().is_success()),
            Err(_) => false,
        }
    }

    /// Kill the engine process if running.
    pub async fn shutdown(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }

    /// Check if the engine has been idle longer than the timeout.
    /// If so, shut it down. Call this from the main tick loop.
    pub async fn check_idle(&mut self) {
        if self.child.is_some() && self.last_used.elapsed() > IDLE_TIMEOUT {
            self.shutdown().await;
        }
    }

    /// Touch the last-used timestamp (called after each translation request).
    pub fn touch(&mut self) {
        self.last_used = Instant::now();
    }
}

impl Drop for LTEngine {
    fn drop(&mut self) {
        // Best-effort synchronous kill. kill_on_drop on the Child also covers this.
        if let Some(ref mut child) = self.child {
            let _ = child.start_kill();
        }
    }
}
