//! Send ONE real transactional email through the configured provider — the only way to know the
//! SMTP settings are right, since every other test in this module is offline (card #165).
//!
//!   SMTP_HOST=smtp.mx.cloudflare.net SMTP_PORT=465 SMTP_USER=api_token \
//!   SMTP_PASSWORD='<token>' MAIL_FROM='Olhar AI <no-reply@olharai.com>' \
//!   cargo run -p app-core --example send_test -- you@example.com
//!
//! A success here means the provider ACCEPTED the message. It does not prove inbox delivery —
//! that is SPF/DKIM/DMARC on the sending domain, and the only way to check is to go and look.
use app_core::notification::{email, templates};

fn main() {
    let to = std::env::args()
        .nth(1)
        .expect("usage: send_test <recipient-email>");
    let mail = templates::password_reset(
        &to,
        "",
        "https://app.olharai.com/reset?t=TEST-NOT-A-REAL-TOKEN",
        30,
    );
    match email::send(&mail) {
        Ok(()) => println!("accepted by the provider → {to} | subject: {}", mail.subject),
        Err(e) => {
            eprintln!("FAILED: {e}");
            std::process::exit(1);
        }
    }
}
