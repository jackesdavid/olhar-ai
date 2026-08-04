//! Transactional email — the account-level messages a self-serve product cannot exist without
//! (card #165).
//!
//! **Why this is separate from `notification`.** That module is the TENANT's alert fabric: workflow
//! graphs deliver to paired beacons, addressed by `chat_id` / beacon label, with image and video
//! keys. These messages are a different thing entirely — they go to ONE person's email address
//! about THEIR account (reset your password, confirm your address, here is your receipt), they are
//! sent by control rather than by a graph, and they must work for a user who has no tenant
//! configuration at all. Forcing them through `notification_queue` would mean inventing a fake
//! beacon for every user.
//!
//! **Why SMTP rather than a provider's REST API.** Resend, SES, Postmark, Mailgun and every other
//! transactional provider speaks SMTP, so this commits us to none of them: point the four env vars
//! at whichever one you sign up for and nothing here changes. A REST integration would have been
//! marginally simpler and would have to be rewritten the first time the provider changed.
//!
//! **Unconfigured is a first-class state, and it is LOUD.** With no `SMTP_HOST`, [`send`] returns
//! [`EmailError::NotConfigured`] rather than pretending to succeed. A silent no-op here would mean
//! password resets that "work" and never arrive — the caller decides what to tell the user.
use std::time::Duration;

/// Where mail goes and who it is from. All four are required; there is no partially-working state.
///
///   SMTP_HOST — e.g. `smtp.resend.com`, `email-smtp.eu-west-1.amazonaws.com`
///   SMTP_PORT — defaults to 587 (STARTTLS). 465 selects implicit TLS.
///   SMTP_USER / SMTP_PASSWORD — the provider's credentials
///   MAIL_FROM — the envelope sender, e.g. `Sentinel <no-reply@olharai.com>`
#[derive(Debug, Clone)]
pub struct EmailConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub from: String,
}

impl EmailConfig {
    /// `None` when email is not configured — the signal callers use to report the feature as
    /// unavailable rather than to fail silently. Mirrors `NotificationConfig::from_env`.
    pub fn from_env() -> Option<Self> {
        let host = non_empty(std::env::var("SMTP_HOST").ok())?;
        let user = std::env::var("SMTP_USER").unwrap_or_default();
        let password = std::env::var("SMTP_PASSWORD").unwrap_or_default();
        let from = non_empty(std::env::var("MAIL_FROM").ok())
            .unwrap_or_else(|| "Sentinel <no-reply@olharai.com>".to_string());
        // An unparseable port is a typo in the deployment, not a reason to silently use 587 and
        // deliver nothing on a provider that needs 465.
        let port = match std::env::var("SMTP_PORT") {
            Ok(p) if !p.trim().is_empty() => p.trim().parse().ok()?,
            _ => 587,
        };
        Some(Self {
            host,
            port,
            user,
            password,
            from,
        })
    }

    /// Port 465 is implicit TLS ("SMTPS"); everything else is STARTTLS on a plaintext connection.
    pub fn implicit_tls(&self) -> bool {
        self.port == 465
    }
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// One message. `text` is required and `html` optional on purpose: a text part always renders, in
/// every client, and it is what keeps a message out of spam folders that penalise html-only mail.
#[derive(Debug, Clone)]
pub struct Email {
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
}

impl Email {
    pub fn new(to: impl Into<String>, subject: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            to: to.into(),
            subject: subject.into(),
            text: text.into(),
            html: None,
        }
    }

    pub fn with_html(mut self, html: impl Into<String>) -> Self {
        self.html = Some(html.into());
        self
    }
}

#[derive(Debug)]
pub enum EmailError {
    /// No `SMTP_HOST` — the feature is off. Distinct from a failure so a caller can say
    /// "email is not set up on this deployment" instead of "something went wrong".
    NotConfigured,
    /// The address could not be parsed, or the message could not be built.
    Invalid(String),
    /// The provider rejected it, or the connection failed.
    Transport(String),
}

impl std::fmt::Display for EmailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => write!(f, "email is not configured (SMTP_HOST unset)"),
            Self::Invalid(e) => write!(f, "invalid email: {e}"),
            Self::Transport(e) => write!(f, "smtp transport failed: {e}"),
        }
    }
}

impl std::error::Error for EmailError {}

/// Send one message, blocking, using the ambient [`EmailConfig::from_env`].
///
/// Blocking to match the rest of app-core's outbound calls (`ureq` everywhere else) — the callers
/// are control's request handlers and the job queue, both of which already block.
pub fn send(email: &Email) -> Result<(), EmailError> {
    let cfg = EmailConfig::from_env().ok_or(EmailError::NotConfigured)?;
    send_with(&cfg, email)
}

/// Send via an explicit config — the form tests and callers with their own config use.
pub fn send_with(cfg: &EmailConfig, email: &Email) -> Result<(), EmailError> {
    use lettre::message::{header::ContentType, MultiPart, SinglePart};
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{Message, SmtpTransport, Transport};

    let from = cfg
        .from
        .parse()
        .map_err(|e| EmailError::Invalid(format!("MAIL_FROM {:?}: {e}", cfg.from)))?;
    let to = email
        .to
        .parse()
        .map_err(|e| EmailError::Invalid(format!("recipient {:?}: {e}", email.to)))?;

    let builder = Message::builder()
        .from(from)
        .to(to)
        .subject(email.subject.clone());

    // multipart/alternative when html is present: clients that render html get it, everything else
    // (and every spam filter) still sees a real text body.
    let msg = match &email.html {
        Some(html) => builder.multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(email.text.clone()),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(html.clone()),
                ),
        ),
        None => builder
            .header(ContentType::TEXT_PLAIN)
            .body(email.text.clone()),
    }
    .map_err(|e| EmailError::Invalid(e.to_string()))?;

    let mut transport = if cfg.implicit_tls() {
        SmtpTransport::relay(&cfg.host).map_err(|e| EmailError::Transport(e.to_string()))?
    } else {
        SmtpTransport::starttls_relay(&cfg.host)
            .map_err(|e| EmailError::Transport(e.to_string()))?
    }
    .port(cfg.port)
    .timeout(Some(Duration::from_secs(20)));

    // Some providers (and a local test relay) take no credentials; sending an empty username is an
    // auth failure rather than an anonymous send, so only authenticate when we actually have one.
    if !cfg.user.is_empty() {
        transport = transport.credentials(Credentials::new(
            cfg.user.clone(),
            cfg.password.clone(),
        ));
    }

    transport
        .build()
        .send(&msg)
        .map_err(|e| EmailError::Transport(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Env is process-global, so these tests must not run concurrently with each other.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_env<T>(vars: &[(&str, Option<&str>)], f: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved: Vec<(String, Option<String>)> = vars
            .iter()
            .map(|(k, _)| (k.to_string(), std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        let out = f();
        for (k, v) in saved {
            match v {
                Some(v) => std::env::set_var(&k, v),
                None => std::env::remove_var(&k),
            }
        }
        out
    }

    /// The property the whole module rests on: no SMTP_HOST means the caller is TOLD, not quietly
    /// handed a success. A silent no-op is a password reset that never arrives.
    #[test]
    fn unconfigured_is_reported_not_swallowed() {
        with_env(&[("SMTP_HOST", None)], || {
            assert!(EmailConfig::from_env().is_none());
            let err = send(&Email::new("a@b.com", "s", "t")).unwrap_err();
            assert!(matches!(err, EmailError::NotConfigured));
        });
    }

    #[test]
    fn reads_config_from_env_with_a_sane_default_port() {
        with_env(
            &[
                ("SMTP_HOST", Some("smtp.resend.com")),
                ("SMTP_PORT", None),
                ("SMTP_USER", Some("resend")),
                ("SMTP_PASSWORD", Some("secret")),
                ("MAIL_FROM", Some("Sentinel <no-reply@olharai.com>")),
            ],
            || {
                let c = EmailConfig::from_env().expect("configured");
                assert_eq!(c.host, "smtp.resend.com");
                assert_eq!(c.port, 587, "default is STARTTLS submission");
                assert!(!c.implicit_tls());
                assert_eq!(c.from, "Sentinel <no-reply@olharai.com>");
            },
        );
    }

    #[test]
    fn port_465_selects_implicit_tls() {
        with_env(
            &[("SMTP_HOST", Some("smtp.example.com")), ("SMTP_PORT", Some("465"))],
            || assert!(EmailConfig::from_env().unwrap().implicit_tls()),
        );
    }

    /// A typo'd port must not silently fall back to 587 and deliver nothing on a 465-only provider.
    #[test]
    fn an_unparseable_port_is_not_configured() {
        with_env(
            &[("SMTP_HOST", Some("smtp.example.com")), ("SMTP_PORT", Some("five-eight-seven"))],
            || assert!(EmailConfig::from_env().is_none()),
        );
    }

    #[test]
    fn a_bad_recipient_is_rejected_before_any_connection() {
        let cfg = EmailConfig {
            host: "127.0.0.1".into(),
            port: 587,
            user: String::new(),
            password: String::new(),
            from: "Sentinel <no-reply@olharai.com>".into(),
        };
        // "not-an-address" cannot parse, so this must fail as Invalid — NOT by dialling a server
        // and timing out, which is how a config typo turns into a 20-second request.
        let err = send_with(&cfg, &Email::new("not-an-address", "s", "t")).unwrap_err();
        assert!(matches!(err, EmailError::Invalid(_)), "got {err:?}");
    }

    #[test]
    fn html_is_optional_and_text_is_always_present() {
        let e = Email::new("a@b.com", "Reset your password", "Use this link: …");
        assert!(e.html.is_none());
        let e = e.with_html("<p>Use this link: …</p>");
        assert!(e.html.is_some());
        assert!(!e.text.is_empty(), "a text part must always exist");
    }
}
