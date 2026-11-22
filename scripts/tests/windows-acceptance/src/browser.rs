//! Drive the already running installed app through WebView2's loopback CDP endpoint.
use anyhow::{Context, Result, bail, ensure};
use headless_chrome::{Browser, Tab, protocol::cdp::Page::CaptureScreenshotFormatOption};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::Child,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(150);
const NORMAL: Duration = Duration::from_secs(30);
const STARTUP: Duration = Duration::from_secs(60);
const REPORT: Duration = Duration::from_secs(240);
// headless_chrome counts browser-level events, not our tab's DOM polling.
// Keep its connection alive throughout the longest quiet period in this suite.
const BROWSER_IDLE: Duration = REPORT.saturating_add(Duration::from_secs(30));
const FAILURE_MARKER: &str = "Acceptance fixture startup failure";

pub struct BrowserPage {
    tab: Arc<Tab>,
    // The remote connection must outlive the tab. An attached Browser does not own the host.
    _browser: Browser,
}

fn app_target(url: &str) -> bool {
    if matches!(url.split_once("://"), Some(("http" | "https", s)) if s.split('/').next()==Some("tauri.localhost"))
    {
        return true;
    }
    url.strip_prefix("http://127.0.0.1:")
        .and_then(|s| s.split('/').next())
        .is_some_and(|s| s.parse::<u16>().is_ok())
}

impl BrowserPage {
    pub fn attach(port: u16, child: &mut Child) -> Result<Self> {
        let endpoint = format!("http://127.0.0.1:{port}/json/version");
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .proxy(None)
            .timeout_global(Some(Duration::from_secs(2)))
            .build()
            .into();
        let deadline = Instant::now() + Duration::from_secs(90);
        let browser = loop {
            ensure!(
                child.try_wait()?.is_none(),
                "Desktop host exited before CDP was ready"
            );
            let attempt = || -> Result<Browser> {
                let body = agent.get(&endpoint).call()?.body_mut().read_to_string()?;
                let info: Value = serde_json::from_str(&body)?;
                let ws = info["webSocketDebuggerUrl"]
                    .as_str()
                    .context("No browser WebSocket URL")?;
                Browser::connect_with_timeout(ws.to_owned(), BROWSER_IDLE)
            };
            match attempt() {
                Ok(browser) => break browser,
                Err(e) if Instant::now() >= deadline => {
                    return Err(e).context("Waiting for WebView2 CDP");
                }
                Err(_) => thread::sleep(POLL),
            }
        };
        loop {
            ensure!(
                child.try_wait()?.is_none(),
                "Desktop host exited before its page appeared"
            );
            let tab = browser
                .get_tabs()
                .lock()
                .expect("CDP tab list poisoned")
                .iter()
                .find(|tab| app_target(&tab.get_url()))
                .cloned();
            if let Some(tab) = tab {
                return Ok(Self {
                    tab,
                    _browser: browser,
                });
            }
            ensure!(
                Instant::now() < deadline,
                "Waiting for the existing packaged or Shiny page"
            );
            thread::sleep(POLL);
        }
    }

    fn value(&self, expression: &str) -> Result<Value> {
        let result = self.tab.evaluate(expression, true)?;
        result
            .value
            .with_context(|| format!("JavaScript returned no value: {:?}", result.description))
    }

    fn until(&self, description: &str, timeout: Duration, expression: &str) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let observed = self.value(expression);
            if matches!(observed, Ok(Value::Bool(true))) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "Timed out waiting for {description} at {}: {observed:?}",
                    self.tab.get_url()
                );
            }
            // The page can change execution contexts during the packaged -> Shiny navigation.
            thread::sleep(POLL);
        }
    }

    fn text(&self, selector: &str, expected: &str, timeout: Duration) -> Result<()> {
        let expr = format!(
            "document.querySelector({})?.innerText.trim() === {}",
            json!(selector),
            json!(expected)
        );
        self.until(&format!("{selector} to show {expected:?}"), timeout, &expr)
    }

    fn visible(&self, selector: &str, text: Option<&str>, timeout: Duration) -> Result<()> {
        let expr = format!(
            r#"[...document.querySelectorAll({selector})].some(el => {{
                const box = el.getBoundingClientRect();
                return box.width > 0 && box.height > 0 && el.checkVisibility({{visibilityProperty:true}})
                    && ({text} === null || el.innerText.trim() === {text});
            }})"#,
            selector = json!(selector),
            text = json!(text)
        );
        self.until(
            &format!("{selector} to be visible ({text:?})"),
            timeout,
            &expr,
        )
    }

    fn click(&self, selector: &str, timeout: Duration) -> Result<()> {
        let expr = format!(
            r#"(() => {{
            const el = document.querySelector({});
            if (!el?.checkVisibility({{visibilityProperty:true}}) || el.matches(':disabled')) return false;
            const aria = el.closest('[aria-disabled="true" i], [aria-disabled="false" i]');
            if (aria?.getAttribute('aria-disabled').toLowerCase() === 'true') return false;
            el.scrollIntoView({{block:'center', inline:'nearest', behavior:'instant'}});
            const b = el.getBoundingClientRect();
            return b.width > 0 && b.height > 0 && el.contains(document.elementFromPoint(b.x+b.width/2,b.y+b.height/2));
        }})()"#,
            json!(selector)
        );
        self.until(&format!("{selector} to be clickable"), timeout, &expr)?;
        // A single trusted mouse click. Retrying a delivered input can increment twice.
        self.tab.find_element(selector)?.click()?;
        Ok(())
    }

    pub fn screenshot(&self, path: &Path) -> Result<()> {
        let png =
            self.tab
                .capture_screenshot(CaptureScreenshotFormatOption::Png, None, None, true)?;
        fs::write(path, png).with_context(|| format!("Writing {}", path.display()))
    }

    pub fn startup(&self, failed: bool, evidence: &Path) -> Result<()> {
        self.visible("[data-stage='load']", None, STARTUP)?;
        self.visible("#startup-events [data-event='r']", None, NORMAL)?;
        self.until(
            "Shiny startup stage",
            NORMAL,
            "document.querySelector('#startup-stage')?.innerText.includes('Shiny')",
        )?;
        self.text("#startup-title", "shinytauri-smoke-minimal", NORMAL)?;
        if !failed {
            let timer = self.value("document.querySelector('#startup-elapsed').innerText")?;
            self.until(
                "startup clock to advance",
                Duration::from_secs(5),
                &format!("document.querySelector('#startup-elapsed').innerText !== {timer}"),
            )?;
            self.screenshot(&evidence.join("minimal-startup.png"))?;
        }
        Ok(())
    }

    pub fn failed(&self, screenshot: &Path, timeout: Option<u64>) -> Result<()> {
        self.visible("[data-stage='failed']", None, STARTUP)?;
        self.visible("#startup-error", None, NORMAL)?;
        ensure!(
            self.value("location.hostname !== '127.0.0.1'")? == true,
            "Failure redirected to the Shiny server"
        );
        ensure!(
            self.value(&format!(
                "!document.body.innerText.includes({})",
                json!(FAILURE_MARKER)
            ))? == true,
            "Raw R failure leaked into the startup page"
        );
        if let Some(seconds) = timeout {
            let matches = self.value(&format!(
                r#"(() => {{
                const msg = document.querySelector('#startup-error').innerText;
                return /(?:^|[^0-9]){seconds}\s*秒/.test(msg) && msg.includes('ポート');
            }})()"#
            ))?;
            ensure!(
                matches == true,
                "startup page omits the configured {seconds} s limit or port phase"
            );
        }
        self.screenshot(screenshot)
    }

    pub fn check_log_links(&self, host: &str, r: &str) -> Result<()> {
        for (selector, name) in [("#host-log", host), ("#r-log", r)] {
            self.until(
                &format!("{selector} to contain the actual log filename"),
                NORMAL,
                &format!(
                    "document.querySelector({})?.innerText.includes({})",
                    json!(selector),
                    json!(name)
                ),
            )?;
        }
        Ok(())
    }

    pub fn exercise(&self, kind: &str, evidence: &Path) -> Result<()> {
        self.until("navigation to the loopback Shiny server", Duration::from_secs(100),
            "location.protocol === 'http:' && location.hostname === '127.0.0.1' && location.port !== ''")?;
        match kind {
            "minimal" => {
                self.text("#runtime", "R 4.5.3", NORMAL)?;
                self.text("#total", "0000 (stringr 1.6.0)", NORMAL)?;
                self.click("#count", NORMAL)?;
                self.text("#total", "0001 (stringr 1.6.0)", NORMAL)?;
                ensure!(
                    self.value(
                        "getComputedStyle(document.querySelector('#total')).fontVariantNumeric"
                    )? == "tabular-nums",
                    "Bundled CSS missing"
                );
            }
            "upgrade" => {
                self.visible(
                    "h1,h2,h3,h4,h5,h6,[role=heading]",
                    Some("Updated counter 0.2.0"),
                    NORMAL,
                )?;
                self.text("#saved", "saved with version 0.1.0", NORMAL)?;
                self.text("#total", "Updated count: 0", NORMAL)?;
                self.click("#count", NORMAL)?;
                self.text("#total", "Updated count: 1", NORMAL)?;
            }
            "rhino" => {
                self.text("#app-counter-count", "カウント：0", NORMAL)?;
                self.click("#app-counter-increment", NORMAL)?;
                self.text("#app-counter-count", "カウント：1", NORMAL)?;
                self.click("#app-dice-roll", NORMAL)?;
                self.click("nav a[href$='#!/report']", NORMAL)?;
                self.click("#app-report-render", STARTUP)?;
                self.visible("#app-report-download", None, REPORT)?;
                self.until("rendered report download URL", REPORT,
                    "document.querySelector('#app-report-download')?.getAttribute('href')?.includes('/download/')")?;
                self.check_report()?;
            }
            _ => bail!("Unknown app fixture {kind}"),
        }
        self.screenshot(&evidence.join(format!("{kind}.png")))
    }

    fn check_report(&self) -> Result<()> {
        // Fetch from the app's own WebView session, keeping the native Save As dialog out of CI.
        let value = self.value(r#"(async () => {
            const response = await fetch(document.querySelector('#app-report-download').href, { signal: AbortSignal.timeout(12000) });
            const disposition = response.headers.get('content-disposition') || '';
            const doc = new DOMParser().parseFromString(await response.text(), 'text/html');
            const texts = selector => Array.from(doc.querySelectorAll(selector), e => e.textContent.trim());
            return JSON.stringify({ok: response.ok, disposition, values: texts('strong'),
                headers: texts('.card-header'), external: doc.querySelectorAll('script[src],link[rel=stylesheet]').length});
        })()"#)?;
        let report: Value =
            serde_json::from_str(value.as_str().context("Missing report snapshot")?)?;
        ensure!(
            report["ok"] == true
                && report["disposition"]
                    .as_str()
                    .is_some_and(|s| s.contains("dice-dashboard.html"))
                && report["external"] == 0,
            "Unexpected report response: {report}"
        );
        for (key, expected) in [
            ("values", "1"),
            ("values", "1,000 回"),
            ("headers", "カウンター"),
            ("headers", "試行回数"),
        ] {
            ensure!(
                report[key]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == expected)),
                "Missing {expected:?} in report: {report}"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        process::{Command, Stdio},
    };

    #[test]
    fn ignores_non_app_targets() {
        assert!(app_target("https://tauri.localhost/index.html"));
        assert!(app_target("http://127.0.0.1:4381/"));
        assert!(!app_target("chrome://new-tab-page/"));
        assert!(!app_target("http://tauri.localhost.example.test/"));
        assert!(!app_target("http://127.0.0.1:4381@example.test/"));
    }

    #[test]
    #[ignore = "Set CHROME to a Chromium executable to exercise a real browser"]
    fn existing_chromium_page() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}/", listener.local_addr()?);
        thread::spawn(move || {
            // Chromium can open and discard speculative connections before the real GET.
            let mut stream = loop {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_millis(500)))
                    .unwrap();
                let mut request = [0u8; 1024];
                if stream.read(&mut request).is_ok_and(|n| n > 0) {
                    break stream;
                }
            };
            let html = r#"<!doctype html><meta charset="utf-8"><link rel="icon" href="data:,">
                <p id="runtime">R 4.5.3</p><p id="total" style="font-variant-numeric:tabular-nums">0000 (stringr 1.6.0)</p>
                <button id="count">count</button><div id="overlay" style="position:fixed;inset:0;background:white"></div>
                <section data-stage="load"><h1 id="startup-title">shinytauri-smoke-minimal</h1>
                <div id="startup-stage">Shiny を起動中</div><p id="startup-elapsed">00:00</p>
                <ol id="startup-events"><li data-event="r">R</li></ol></section>
                <script>let t=0;setInterval(()=>document.getElementById('startup-elapsed').textContent=++t,250);
                setTimeout(()=>overlay.remove(),600);count.onclick=e=>{
                if(e.isTrusted) setTimeout(()=>total.textContent='0001 (stringr 1.6.0)',175);};</script>"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",html.len()).unwrap();
        });
        let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let mut command = Command::new(
            std::env::var_os("CHROME").context("Set CHROME to a Chromium executable")?,
        );
        command
            .args([
                "--headless",
                "--disable-gpu",
                "--no-first-run",
                "--disable-dev-shm-usage",
            ])
            .arg(format!("--remote-debugging-port={port}"))
            .arg(format!("--user-data-dir={}", tmp.path().display()));
        if cfg!(target_os = "linux") {
            command.arg("--no-sandbox");
        }
        let mut child = command
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let test = (|| -> Result<()> {
            let page = BrowserPage::attach(port, &mut child)?;
            page.startup(false, tmp.path())?;
            page.until(
                "fixture overlay removed",
                NORMAL,
                "!document.getElementById('overlay')",
            )?;
            page.value("document.body.setAttribute('aria-disabled', 'true'); true")?;
            ensure!(
                page.click("#count", Duration::from_millis(250)).is_err(),
                "Clicked a button in a disabled group"
            );
            page.value("document.body.removeAttribute('aria-disabled'); true")?;
            page.exercise("minimal", tmp.path())?;
            ensure!(fs::read(tmp.path().join("minimal.png"))?.starts_with(b"\x89PNG\r\n\x1a\n"));
            let err = page
                .text("#total", "incorrect counter", Duration::from_millis(300))
                .unwrap_err();
            ensure!(
                err.to_string().contains("Timed out waiting for #total"),
                "{err}"
            );
            // A held R startup can produce no browser-level events for over
            // 15 s even while the driver polls this tab's DOM. Navigation must
            // still work when the host finally changes pages.
            page.value("setTimeout(() => location.assign('about:blank'), 17000); true")?;
            page.until(
                "navigation after a quiet startup page",
                NORMAL,
                "location.href === 'about:blank'",
            )?;
            page.value(r#"document.body.innerHTML = "<main data-stage='failed'><p id='startup-error'>Startup failed</p><p id='host-log'>C:/logs/host-21.jsonl</p><p id='r-log'>C:/logs/shiny-21.log</p></main>""#)?;
            page.value(r#"document.body.insertAdjacentHTML('beforeend', '<h2 style="height:0;overflow:hidden">Updated counter 0.2.0</h2>'); document.querySelector('#startup-error').style.cssText='width:0;overflow:hidden'"#)?;
            for (selector, text) in [
                ("#startup-error", None),
                ("h2", Some("Updated counter 0.2.0")),
            ] {
                ensure!(
                    page.visible(selector, text, Duration::from_millis(250))
                        .is_err(),
                    "{selector} has an empty bounding box"
                );
            }
            page.value("document.querySelector('#startup-error').removeAttribute('style'); true")?;
            page.failed(&tmp.path().join("intentional-failure.png"), None)?;
            page.check_log_links("host-21.jsonl", "shiny-21.log")?;
            let timeout_png = tmp.path().join("startup-timeout.png");
            page.value("document.querySelector('#startup-error').textContent = '20 秒以内にポートを受け取れませんでした'")?;
            page.failed(&timeout_png, Some(20))?;
            ensure!(
                page.failed(&timeout_png, Some(0)).is_err(),
                "accepted the wrong UI timeout"
            );
            page.value(&format!(
                "document.getElementById('startup-error').textContent = {}",
                json!(FAILURE_MARKER)
            ))?;
            ensure!(
                page.failed(&tmp.path().join("intentional-failure.png"), None)
                    .unwrap_err()
                    .to_string()
                    .contains("Raw R failure")
            );
            drop(page);
            ensure!(
                child.try_wait()?.is_none(),
                "Disconnecting closed the external browser"
            );
            Ok(())
        })();
        let _ = child.kill();
        let _ = child.wait();
        test
    }
}
