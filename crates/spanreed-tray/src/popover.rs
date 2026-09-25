//! Borderless usage card shown from the tray icon on macOS, Windows, and Linux.

use std::sync::mpsc::{self, Receiver, Sender};

use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event_loop::EventLoopWindowTarget;
use tao::window::{Window, WindowBuilder};
use tray_icon::Rect;
use wry::WebViewBuilder;

pub struct Popover {
    window: Window,
    webview: wry::WebView,
    rx: Receiver<String>,
    visible: bool,
    opened: std::time::Instant,
}

impl Popover {
    pub fn new(target: &EventLoopWindowTarget<()>) -> Result<Self, String> {
        let window = WindowBuilder::new()
            .with_title("Spanreed")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_always_on_top(true)
            .with_inner_size(LogicalSize::new(392.0, 720.0))
            .build(target)
            .map_err(|error| format!("usage card window: {error}"))?;
        // Visibility changes wait for the event loop, so a hidden window still
        // has no native handle here. Realize it directly or the card view
        // cannot attach and the tray exits.
        #[cfg(target_os = "linux")]
        {
            use gtk::prelude::WidgetExt;
            use tao::platform::unix::WindowExtUnix;
            window.gtk_window().realize();
        }
        let (tx, rx) = mpsc::channel();
        let webview = build_view(&window, tx)?;
        Ok(Self {
            window,
            webview,
            rx,
            visible: false,
            opened: std::time::Instant::now(),
        })
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn toggle(&mut self, anchor: Rect, html: String) {
        if self.visible {
            self.hide();
            return;
        }
        self.show(anchor, html);
    }

    pub fn show(&mut self, anchor: Rect, html: String) {
        self.load(html);
        let width = 392.0;
        let height = 720.0;
        let x = anchor.position.x.round();
        let below = anchor.position.y + f64::from(anchor.size.height) + 8.0;
        let y = if anchor.position.y < 120.0 {
            below
        } else {
            (anchor.position.y - height - 8.0).max(0.0)
        };
        let x = (x - width + f64::from(anchor.size.width)).max(8.0);
        self.window.set_outer_position(PhysicalPosition::new(x, y));
        self.window.set_visible(true);
        self.window.set_focus();
        self.visible = true;
        self.opened = std::time::Instant::now();
    }

    pub fn hide(&mut self) {
        self.window.set_visible(false);
        self.visible = false;
    }

    pub fn load(&self, html: String) {
        self.webview.load_html(&html).ok();
    }

    /// Update the status line in the open document. A full reload can finish
    /// after the line was already cleared, which left it on screen.
    pub fn sync_status(&self, status: Option<&str>) {
        let text = serde_json::to_string(status.unwrap_or("")).unwrap_or_else(|_| "\"\"".into());
        let script = format!(
            r#"(function() {{
              var text = {text};
              var node = document.querySelector("p.status");
              if (!text) {{ if (node) node.remove(); return; }}
              if (!node) {{
                node = document.createElement("p");
                node.className = "status";
                var card = document.querySelector("main.card");
                if (card) card.insertBefore(node, card.firstChild);
              }}
              node.textContent = text;
            }})()"#
        );
        self.webview.evaluate_script(&script).ok();
    }

    pub fn poll(&self) -> Option<String> {
        self.rx.try_recv().ok()
    }

    /// Ignore the blur that opening the window itself can emit.
    pub fn blur_should_close(&self) -> bool {
        self.visible && self.opened.elapsed() > std::time::Duration::from_millis(350)
    }
}

fn build_view(window: &Window, tx: Sender<String>) -> Result<wry::WebView, String> {
    WebViewBuilder::new()
        .with_html("<html><body></body></html>")
        .with_ipc_handler(move |request| {
            if let Some(message) = request.body().lines().next() {
                let _ = tx.send(message.trim().to_string());
            }
        })
        .build(window)
        .map_err(|error| format!("usage card view: {error}"))
}

/// Accept only https links this card itself generated.
pub fn allowed_buy_url(url: &str) -> bool {
    matches!(
        url,
        "https://chatgpt.com/"
            | "https://claude.ai/settings/billing"
            | "https://grok.com/"
            | "https://github.com/settings/copilot"
    )
}
