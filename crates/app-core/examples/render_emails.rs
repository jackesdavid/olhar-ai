//! Render the transactional emails to HTML files so a human can LOOK at them (card #165).
//!
//! Assertions prove a link is present; they cannot tell you the thing is well made. Run this and
//! open the files in a browser:
//!
//!   cargo run -p app-core --example render_emails -- /tmp/emails
use app_core::notification::templates;

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| {
        std::env::temp_dir()
            .join("sentinel-emails")
            .display()
            .to_string()
    });
    std::fs::create_dir_all(&dir).expect("create output dir");

    let app = "https://app.olharai.com";
    // Both languages, side by side. Rendering only English is how the Portuguese half of a
    // bilingual product ships unlooked-at.
    for lang in [templates::Lang::En, templates::Lang::Pt] {
        let tag = lang.as_tag();
        let samples = [
            (
                "welcome",
                templates::welcome("jo@example.com", "Jo", app, lang),
            ),
            (
                "password-reset",
                templates::password_reset(
                    "jo@example.com",
                    "Jo",
                    &format!("{app}/reset?t=8f3c1a"),
                    30,
                    lang,
                ),
            ),
            (
                "password-changed",
                templates::password_changed(
                    "jo@example.com",
                    "Jo",
                    "3 August 2026 at 21:04",
                    &format!("{app}/help"),
                    lang,
                ),
            ),
        ];
        for (name, email) in samples {
            let path = format!("{dir}/{name}.{tag}.html");
            let html = email.html.clone().unwrap_or_default();
            std::fs::write(&path, &html).expect("write html");
            std::fs::write(format!("{dir}/{name}.{tag}.txt"), &email.text).expect("write text");
            println!("{path}  —  subject: {}", email.subject);
        }
    }
}
