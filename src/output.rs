use std::io::IsTerminal;

/// Terminal rendering helper. All visual decorations are skipped when color
/// is disabled, so the output stays pipe-friendly.
pub struct Output {
    color: bool,
}

impl Output {
    pub fn stdout() -> Output {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        Output {
            color: std::io::stdout().is_terminal() && !no_color,
        }
    }

    pub fn with_color(color: bool) -> Output {
        Output { color }
    }

    fn paint(&self, text: &str, code: &str) -> String {
        if self.color && !text.is_empty() {
            format!("\x1b[{}m{}\x1b[0m", code, text)
        } else {
            text.to_string()
        }
    }

    pub fn dim(&self, text: &str) -> String {
        self.paint(text, "2")
    }

    pub fn accent(&self, text: &str) -> String {
        self.paint(text, "1;36")
    }

    pub fn title(&self, text: &str) {
        println!("{}", self.accent(text));
    }

    pub fn section(&self, title: &str) {
        println!();
        println!(
            "{}",
            self.paint(&format!(" {} ", title.to_uppercase()), "1;97;44")
        );
    }

    pub fn kv(&self, key: &str, value: &str) {
        println!("  {} {}", self.dim(&format!("{:<10}", key)), value);
    }

    pub fn kv_lines(&self, key: &str, values: &[String]) {
        let Some((first, rest)) = values.split_first() else {
            return;
        };
        self.kv(key, first);
        for extra in rest {
            println!("  {} {}", self.dim(&" ".repeat(10)), extra);
        }
    }

    pub fn note(&self, text: &str) {
        println!("  {}", self.dim(text));
    }

    /// Colored percentage text, padded to `width` columns.
    pub fn pct_cell(&self, percent: f64, width: usize) -> String {
        let text = format!("{:.0}%", percent);
        let pad = width.saturating_sub(text.len());
        format!(
            "{}{}",
            " ".repeat(pad),
            self.paint(&text, pct_code(percent))
        )
    }

    /// A 0-100% usage bar such as `████░░░░░░`.
    pub fn bar(&self, percent: f64, width: usize) -> String {
        let clamped = percent.clamp(0.0, 100.0);
        let filled = (((clamped / 100.0) * width as f64).round() as usize).min(width);
        let mut out = self.paint(&"█".repeat(filled), pct_code(clamped));
        out.push_str(&self.paint(&"░".repeat(width - filled), "2"));
        out
    }
}

fn pct_code(percent: f64) -> &'static str {
    if percent >= 90.0 {
        "31"
    } else if percent >= 70.0 {
        "33"
    } else {
        "32"
    }
}
