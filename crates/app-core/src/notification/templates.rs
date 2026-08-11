//! The transactional messages themselves (card #165) — welcome, password reset, password changed.
//!
//! Each returns a ready [`Email`] with **both** a text and an html part. Text is not a fallback
//! afterthought: it renders in every client, and html-only mail is penalised by spam filters —
//! which for a password reset means the customer never gets back into their account.
//!
//! ## Carrying the identity into a medium that fights you
//!
//! The brand is the **eclipse-eye in orbit** (`assets/BRAND.md`): an eclipsed disc with a pupil,
//! inside a dotted orbit ring carrying a small moon. *"Olhar"* is Portuguese for *gaze* — the
//! product watches, and the mark is an eye.
//!
//! Email cannot use the `OlharMark` component, cannot use inline SVG (Gmail strips `<svg>`, Outlook
//! will not render it), and cannot rely on an image — **images are blocked by default in most
//! clients**, so a hosted logo arrives as a broken box in the one position where the brand should
//! be. The mark is therefore built from nested `<div>`s with `border-radius`, geometry scaled from
//! BRAND.md's 120-unit viewBox. In clients that ignore `border-radius` (Outlook's Word engine) it
//! degrades to squares in the right places at the right sizes — still the composition, still
//! monochrome, never broken.
//!
//! The **wordmark is text**, not an image: OLHAR AI, Sora 600, uppercase, 0.16em tracking. It
//! therefore renders even where images are blocked, and the uppercase + tracking carries the
//! identity in clients that will not load Sora.
//!
//! Everything else follows the ink theme: soft-gray canvas, white card lifting off it, one
//! near-black ink, **no brand hue**. The only colour is `--destructive` red on the "this was not
//! me" path, which survives because it communicates STATE, not brand.
//!
//! No external stylesheet, no `<style>` block, no web fonts, no images. Every rule is inline.
use super::email::Email;

/// The product name as customers see it. Lives in [`crate::brand`] now — the emails were the only
/// place that had it right, and the rest of the product needed the same single source.
use crate::brand::PRODUCT;
/// Origin for images referenced by email. Overridable so a staging deployment does not hotlink
/// production, and so the mark follows the app if the domain moves.
fn asset_base() -> String {
    std::env::var("APP_ORIGIN")
        .or_else(|_| std::env::var("WEBAUTHN_RP_ORIGIN"))
        .ok()
        .map(|s| s.trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://app.olharai.com".to_string())
}

/// Which language an account email is written in (#167 follow-up).
///
/// The product sells in Brazil; a Portuguese-speaking customer receiving an English password reset
/// at the moment they are locked out is the worst possible time to be spoken to in the wrong
/// language. Stored per user (`users.locale`) rather than taken from the request, because the
/// emails that matter most later — trial expiry, receipts, dunning — are sent by a scheduler with
/// no browser anywhere near them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    /// The fallback: matches the app's own default when the browser is not Portuguese.
    #[default]
    En,
    Pt,
}

impl Lang {
    /// Parse a BCP-47-ish tag (`pt`, `pt-BR`, `en-US`). Anything unrecognised is [`Lang::En`] —
    /// an unknown locale must produce a real email in a plausible language, never no email.
    pub fn from_tag(tag: &str) -> Lang {
        if tag.trim().to_ascii_lowercase().starts_with("pt") {
            Lang::Pt
        } else {
            Lang::En
        }
    }

    /// Parse a stored `users.locale`, which is NULL for every account created before it existed.
    /// `None` is the documented fallback path, not an error.
    pub fn from_opt_tag(tag: Option<&str>) -> Lang {
        tag.map(Lang::from_tag).unwrap_or_default()
    }

    /// The tag stored on the user row.
    pub fn as_tag(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Pt => "pt",
        }
    }

    /// Pick between an English and a Portuguese literal. Keeps the HTML structure single-sourced:
    /// only the words vary, so the two languages cannot drift in layout.
    fn pick<T>(self, en: T, pt: T) -> T {
        match self {
            Lang::En => en,
            Lang::Pt => pt,
        }
    }
}

/// Ink theme, resolved to literals because email has no custom properties.
const INK: &str = "#18181b"; // --primary / --foreground (near-black)
const INK_SOFT: &str = "#3f3f46"; // body copy
const INK_MUTED: &str = "#71717a"; // --muted-foreground
const CANVAS: &str = "#f6f6f7"; // page behind the card
const CARD_BORDER: &str = "#e6e6e8";
const DANGER: &str = "#b42318"; // --destructive, state only

/// The dot-field mark: one bright focal dot with a ring of six, in hexagonal packing.
///
/// The brand mark — the real asset, served over HTTPS.
///
/// **Why an image, after trying not to.** The first version of this drew the mark from nested
/// `<div>`s with `border-radius` and absolute positioning, to survive the image blocking most
/// clients apply by default. It did not survive: clients strip `position:absolute`, so the five
/// layers that were supposed to overlap stacked vertically instead and arrived as a column of
/// grey blobs. No arrangement of CSS boxes renders an eclipsed crescent with a pupil and an
/// orbiting moon — the shape needs real vector art, and inline `<svg>` is stripped by Gmail and
/// ignored by Outlook's Word engine.
///
/// So: the canonical app icon, by URL. That is what every transactional provider does, and it is
/// the only way the mark in the mail is the SAME artwork as the mark in the product rather than an
/// approximation that drifts. `alt` carries the brand when images are blocked, and the wordmark
/// below is real text, so a blocked-image reader still sees OLHAR AI.
fn brand_mark() -> String {
    let base = asset_base();
    format!(
        r#"    <div style="text-align:center;margin:0 0 14px;">
      <img src="{base}/icon-192.png" width="56" height="56" alt="Olhar AI"
           style="width:56px;height:56px;border-radius:13px;display:inline-block;border:0;outline:none;text-decoration:none;" />
    </div>"#
    )
}

/// The wordmark: **OLHAR AI**, Sora 600, uppercase, 0.16em tracking (`assets/BRAND.md` § Typography).
///
/// Text, never an image — it renders in every client including the ones that block images, and it
/// is what keeps the header legible when the mark above does not load. Sora is named first for the
/// clients that honour web fonts; everywhere else the uppercase + tracking still reads as the
/// wordmark rather than as body copy, which is why the treatment matters more than the face.
fn wordmark() -> String {
    format!(
        r#"    <div style="text-align:center;font-family:'Sora','Helvetica Neue',Helvetica,Arial,sans-serif;font-weight:600;font-size:15px;letter-spacing:0.16em;text-transform:uppercase;color:{INK};margin:0 0 30px;">OLHAR AI</div>"#
    )
}

/// The shared shell: dot-field mark, eyebrow, heading, body, footer — centred, calm, generous.
///
/// `preheader` is the grey line clients show beside the subject in the inbox list. Without one they
/// show the first words of the body, which is the greeting and tells the reader nothing.
fn shell(preheader: &str, eyebrow: &str, heading: &str, body: &str, lang: Lang) -> String {
    let tagline = lang.pick(
        "camera monitoring and automation",
        "monitoramento de câmeras e automação",
    );
    format!(
        r#"<div style="margin:0;padding:32px 12px;background:{CANVAS};">
  <span style="display:none!important;visibility:hidden;opacity:0;height:0;width:0;overflow:hidden;">{preheader}</span>
  <div style="max-width:560px;margin:0 auto;background:#ffffff;border:1px solid {CARD_BORDER};border-radius:14px;padding:40px 36px;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;color:{INK};font-size:15px;line-height:1.6;">
{}
    <div style="font-size:11px;font-weight:600;text-transform:uppercase;letter-spacing:0.08em;color:{INK_MUTED};margin:0 0 10px;">{eyebrow}</div>
    <div style="font-size:21px;font-weight:600;letter-spacing:-0.01em;line-height:1.3;margin:0 0 20px;">{heading}</div>
{body}
  </div>
  <div style="max-width:560px;margin:20px auto 0;text-align:center;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;font-size:12px;line-height:1.5;color:{INK_MUTED};">
    {PRODUCT} — {tagline}
  </div>
</div>"#,
        format!("{}\n{}", brand_mark(), wordmark())
    )
}

/// The primary action. A real `<a>` styled as a button — a `<button>` does nothing in mail — and
/// the destination is ALWAYS printed as a plain link too, because some clients strip the styling
/// and some readers simply will not click a button.
fn button(href: &str, label: &str, lang: Lang) -> String {
    let fallback = lang.pick(
        "If the button does not work, paste this into your browser:",
        "Se o botão não funcionar, cole este endereço no seu navegador:",
    );
    format!(
        r#"    <div style="margin:28px 0 20px;">
      <a href="{href}" style="display:inline-block;background:{INK};color:#ffffff;text-decoration:none;padding:13px 24px;border-radius:10px;font-weight:600;font-size:15px;line-height:1;">{label}</a>
    </div>
    <div style="font-size:13px;color:{INK_MUTED};margin:0 0 6px;">{fallback}</div>
    <div style="font-size:13px;word-break:break-all;margin:0 0 24px;"><a href="{href}" style="color:{INK};">{href}</a></div>"#
    )
}

/// Sent once, right after signup. Its job is the FIRST ACTION, not a greeting: a monitoring product
/// is worth nothing until a camera is attached, and this is the moment of highest intent.
pub fn welcome(to: &str, name: &str, app_url: &str, lang: Lang) -> Email {
    let name = display_name(name, to);
    let (s1, s2, s3) = lang.pick(
        (
            "Install the appliance on a machine that stays on at home or at the site.",
            "Pair it with the code the app shows you.",
            "Add a camera — {PRODUCT} finds most of them on the network by itself.",
        ),
        (
            "Instale o appliance em uma máquina que fique ligada em casa ou no local.",
            "Faça o pareamento com o código que o app mostra.",
            "Adicione uma câmera — o {PRODUCT} encontra a maioria delas sozinho na rede.",
        ),
    );
    let (s1, s2, s3) = (
        s1.replace("{PRODUCT}", PRODUCT),
        s2.replace("{PRODUCT}", PRODUCT),
        s3.replace("{PRODUCT}", PRODUCT),
    );
    let text = lang.pick(
        format!(
            "Welcome to {PRODUCT}, {name}.\n\n\
             Your account is ready and your trial has started.\n\n\
             To see anything useful, connect your first camera:\n\
             1. {s1}\n\
             2. {s2}\n\
             3. {s3}\n\n\
             Start here: {app_url}\n\n\
             If you get stuck, reply to this email.\n"
        ),
        format!(
            "Bem-vindo ao {PRODUCT}, {name}.\n\n\
             Sua conta está pronta e seu período de teste começou.\n\n\
             Para ver algo útil, conecte sua primeira câmera:\n\
             1. {s1}\n\
             2. {s2}\n\
             3. {s3}\n\n\
             Comece por aqui: {app_url}\n\n\
             Se travar em algo, responda este e-mail.\n"
        ),
    );
    let html = shell(
        lang.pick(
            "Connect your first camera to finish setting up.",
            "Conecte sua primeira câmera para concluir a configuração.",
        ),
        lang.pick("Welcome", "Bem-vindo"),
        &lang.pick(format!("You're in, {name}."), format!("Tudo pronto, {name}.")),
        &format!(
            r#"    <div style="margin:0 0 18px;color:{INK_SOFT};">{intro}</div>
    <div style="margin:0 0 6px;color:{INK_SOFT};"><strong style="color:{INK};">1.</strong> {s1}</div>
    <div style="margin:0 0 6px;color:{INK_SOFT};"><strong style="color:{INK};">2.</strong> {s2}</div>
    <div style="margin:0;color:{INK_SOFT};"><strong style="color:{INK};">3.</strong> {s3}</div>
{}
    <div style="font-size:13px;color:{INK_MUTED};">{outro}</div>"#,
            button(
                app_url,
                lang.pick("Set up my first camera", "Configurar minha primeira câmera"),
                lang,
            ),
            intro = lang.pick(
                "Your account is ready and your trial has started. To see anything useful, connect your first camera:",
                "Sua conta está pronta e seu período de teste começou. Para ver algo útil, conecte sua primeira câmera:",
            ),
            outro = lang.pick(
                "If you get stuck, just reply to this email.",
                "Se travar em algo, é só responder este e-mail.",
            ),
        ),
        lang,
    );
    Email::new(
        to,
        lang.pick(
            format!("Welcome to {PRODUCT}"),
            format!("Bem-vindo ao {PRODUCT}"),
        ),
        text,
    )
    .with_html(html)
}

/// The one that decides whether a locked-out customer comes back or churns. States the expiry
/// plainly — a reset link that has quietly expired is indistinguishable from one that never worked.
pub fn password_reset(
    to: &str,
    name: &str,
    reset_url: &str,
    valid_minutes: u32,
    lang: Lang,
) -> Email {
    let name = display_name(name, to);
    let text = lang.pick(
        format!(
            "Hello {name},\n\n\
             Someone asked to reset the password for your {PRODUCT} account ({to}).\n\n\
             Set a new password here — the link works for {valid_minutes} minutes:\n\
             {reset_url}\n\n\
             If this was not you, you can ignore this email. Your password stays as it is, \
             and nobody can use this link without opening it from your inbox.\n"
        ),
        format!(
            "Olá {name},\n\n\
             Alguém pediu a redefinição da senha da sua conta {PRODUCT} ({to}).\n\n\
             Defina uma nova senha aqui — o link vale por {valid_minutes} minutos:\n\
             {reset_url}\n\n\
             Se não foi você, pode ignorar este e-mail. Sua senha continua a mesma, \
             e ninguém consegue usar este link sem abri-lo na sua caixa de entrada.\n"
        ),
    );
    let html = shell(
        &lang.pick(
            format!("Reset your password — the link is valid for {valid_minutes} minutes."),
            format!("Redefina sua senha — o link vale por {valid_minutes} minutos."),
        ),
        lang.pick("Account", "Conta"),
        lang.pick("Reset your password", "Redefina sua senha"),
        &format!(
            r#"    <div style="margin:0 0 14px;color:{INK_SOFT};">{line1}</div>
    <div style="margin:0;color:{INK_SOFT};">{line2}</div>
{}
    <div style="font-size:13px;color:{INK_MUTED};">{outro}</div>"#,
            button(
                reset_url,
                lang.pick("Set a new password", "Definir nova senha"),
                lang,
            ),
            line1 = lang.pick(
                format!(r#"Hello {name} — someone asked to reset the password for <strong style="color:{INK};">{to}</strong>."#),
                format!(r#"Olá {name} — alguém pediu a redefinição da senha de <strong style="color:{INK};">{to}</strong>."#),
            ),
            line2 = lang.pick(
                format!(r#"The link below works for <strong style="color:{INK};">{valid_minutes} minutes</strong>."#),
                format!(r#"O link abaixo vale por <strong style="color:{INK};">{valid_minutes} minutos</strong>."#),
            ),
            outro = lang.pick(
                "If this was not you, ignore this email — your password stays as it is, and nobody can use this link without opening it from your inbox.",
                "Se não foi você, ignore este e-mail — sua senha continua a mesma, e ninguém consegue usar este link sem abri-lo na sua caixa de entrada.",
            ),
        ),
        lang,
    );
    Email::new(
        to,
        lang.pick(
            format!("Reset your {PRODUCT} password"),
            format!("Redefina sua senha do {PRODUCT}"),
        ),
        text,
    )
    .with_html(html)
}

/// Sent AFTER a password changes, to the address on the account. This exists for exactly one
/// reader: the person whose account was just taken. Everyone else deletes it, which is fine — the
/// mail costs nothing and not sending it costs a silent account takeover.
pub fn password_changed(
    to: &str,
    name: &str,
    when: &str,
    support_url: &str,
    lang: Lang,
) -> Email {
    let name = display_name(name, to);
    let text = lang.pick(
        format!(
            "Hello {name},\n\n\
             The password for your {PRODUCT} account ({to}) was changed on {when}.\n\n\
             If you made this change, there is nothing to do.\n\n\
             If you did not change it, act now — someone else may have access to your account \
             and your cameras:\n\
             {support_url}\n"
        ),
        format!(
            "Olá {name},\n\n\
             A senha da sua conta {PRODUCT} ({to}) foi alterada em {when}.\n\n\
             Se foi você, não há nada a fazer.\n\n\
             Se não foi você, aja agora — outra pessoa pode ter acesso à sua conta \
             e às suas câmeras:\n\
             {support_url}\n"
        ),
    );
    let html = shell(
        lang.pick(
            "Your password was changed. If this was not you, act now.",
            "Sua senha foi alterada. Se não foi você, aja agora.",
        ),
        lang.pick("Security", "Segurança"),
        lang.pick("Your password was changed", "Sua senha foi alterada"),
        &format!(
            r#"    <div style="margin:0 0 14px;color:{INK_SOFT};">{line1}</div>
    <div style="margin:0 0 18px;color:{INK_SOFT};">{line2}</div>
    <div style="border-left:3px solid {DANGER};padding:2px 0 2px 14px;margin:0;color:{INK_SOFT};">{warn}</div>
{}"#,
            button(
                support_url,
                lang.pick("Secure my account", "Proteger minha conta"),
                lang,
            ),
            line1 = lang.pick(
                format!(r#"Hello {name} — the password for <strong style="color:{INK};">{to}</strong> was changed on <strong style="color:{INK};">{when}</strong>."#),
                format!(r#"Olá {name} — a senha de <strong style="color:{INK};">{to}</strong> foi alterada em <strong style="color:{INK};">{when}</strong>."#),
            ),
            line2 = lang.pick(
                "If you made this change, there is nothing to do.",
                "Se foi você, não há nada a fazer.",
            ),
            warn = lang.pick(
                format!(r#"If you did <strong style="color:{DANGER};">not</strong> change it, act now — someone else may have access to your account and your cameras."#),
                format!(r#"Se <strong style="color:{DANGER};">não</strong> foi você, aja agora — outra pessoa pode ter acesso à sua conta e às suas câmeras."#),
            ),
        ),
        lang,
    );
    Email::new(
        to,
        lang.pick(
            format!("Your {PRODUCT} password was changed"),
            format!("Sua senha do {PRODUCT} foi alterada"),
        ),
        text,
    )
    .with_html(html)
}

/// Address the reader by name, falling back to the local part of their email. Never "Hello ,".
fn display_name(name: &str, email: &str) -> String {
    let n = name.trim();
    if !n.is_empty() {
        return n.to_string();
    }
    email
        .split('@')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("there")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<Email> {
        vec![
            welcome("jo@example.com", "Jo", "https://app.olharai.com", Lang::En),
            password_reset("jo@example.com", "Jo", "https://app.olharai.com/reset?t=abc", 30, Lang::En),
            password_changed(
                "jo@example.com",
                "Jo",
                "3 August 2026 at 21:04",
                "https://app.olharai.com/help",
                Lang::En,
            ),
        ]
    }

    /// Every message carries BOTH parts. An html-only transactional mail is the one that lands in
    /// spam, and for a password reset that is a customer who cannot get back in.
    #[test]
    fn every_template_has_a_text_and_an_html_part() {
        for e in all() {
            assert!(!e.subject.trim().is_empty(), "subject empty");
            assert!(e.text.len() > 40, "text part too thin: {:?}", e.text);
            assert!(e.html.expect("html part").contains("<div"));
        }
    }

    /// Email HTML gets ONE external asset — the logo — and nothing else.
    ///
    /// `<style>`, `<link>`, `background-image` and `@font-face` are still banned outright: clients
    /// strip them, and anything that depends on them is broken before it is read. The logo is the
    /// deliberate exception, because the mark cannot be drawn in CSS boxes (an earlier attempt
    /// arrived as a column of grey blobs when the client stripped `position:absolute`) and inline
    /// `<svg>` is removed by Gmail. It is allowed exactly once, over HTTPS, with `alt` text.
    #[test]
    fn the_only_external_asset_is_the_logo() {
        for e in all() {
            let html = e.html.clone().unwrap_or_default();
            for banned in ["<style", "<link", "background-image", "@font-face"] {
                assert!(!html.contains(banned), "{banned:?} must not appear in email html");
            }
            assert_eq!(
                html.matches("<img").count(),
                1,
                "exactly one image — the logo — is allowed"
            );
            assert_eq!(html.matches("src=").count(), 1, "no other external source");
            assert!(
                html.contains("src=\"https://"),
                "the logo must load over HTTPS"
            );
            assert!(
                html.contains("alt=\"Olhar AI\""),
                "a blocked image must still say who sent this"
            );
        }
    }

    /// The mark must be the REAL artwork, not an approximation.
    ///
    /// Two previous versions of this test passed against the wrong logo: one asserted seven dots
    /// long after the dot-field was retired, and one asserted a CSS construction that rendered as
    /// grey blobs in a real client. Both were green. So this asserts the thing that cannot drift —
    /// that the mail points at the same file the product ships as its icon.
    #[test]
    fn the_mark_is_the_shipped_app_icon() {
        for e in all() {
            let html = e.html.clone().unwrap_or_default();
            assert!(
                html.contains("/icon-192.png"),
                "the mark must be the app icon the product actually ships"
            );
            assert!(
                html.contains("width=\"56\"") && html.contains("height=\"56\""),
                "width/height attributes are required — Outlook ignores CSS sizing and will \
                 otherwise render the source at 192px"
            );
        }
    }

    /// The wordmark must be TEXT: it is the half of the lockup that survives blocked images.
    #[test]
    fn the_wordmark_is_text_with_brand_typography() {
        for e in all() {
            let html = e.html.unwrap_or_default();
            assert!(html.contains(">OLHAR AI<"), "wordmark must be real text");
            assert!(html.contains("letter-spacing:0.16em"), "BRAND.md tracking");
            assert!(html.contains("text-transform:uppercase"), "BRAND.md case");
            assert!(html.contains("'Sora'"), "Sora named first in the stack");
        }
    }

    /// No brand hue on surfaces or the primary action — the button is ink, per the theme.
    #[test]
    fn the_primary_action_is_ink_not_a_brand_colour() {
        let e = welcome("jo@example.com", "Jo", "https://x", Lang::En);
        let html = e.html.unwrap();
        assert!(html.contains(&format!("background:{INK};color:#ffffff")));
        for hue in ["#10b981", "emerald", "#22c55e"] {
            assert!(!html.contains(hue), "retired brand hue {hue} reintroduced");
        }
    }

    /// Colour survives only where it means STATE. Red appears on the takeover path and nowhere else.
    #[test]
    fn danger_colour_is_used_only_for_the_security_path() {
        assert!(welcome("j@e.com", "Jo", "https://x", Lang::En).html.unwrap().find(DANGER).is_none());
        assert!(password_reset("j@e.com", "Jo", "https://x", 30, Lang::En).html.unwrap().find(DANGER).is_none());
        assert!(password_changed("j@e.com", "Jo", "now", "https://x", Lang::En).html.unwrap().contains(DANGER));
    }

    /// A template that ships an unsubstituted placeholder tells the customer the product is broken
    /// at the exact moment they needed it to work.
    #[test]
    fn no_unsubstituted_placeholders_survive() {
        for e in all() {
            let html = e.html.unwrap_or_default();
            for body in [&e.text, &html] {
                for marker in ["{}", "{{", "}}", "{name}", "{to}"] {
                    assert!(!body.contains(marker), "placeholder {marker:?} left in: {body}");
                }
            }
        }
    }

    /// The link is the payload. It must appear in the text part too — that is what renders when the
    /// html is stripped.
    #[test]
    fn the_action_link_appears_in_both_parts() {
        let url = "https://app.olharai.com/reset?t=abc";
        let e = password_reset("jo@example.com", "Jo", url, 30, Lang::En);
        assert!(e.text.contains(url), "reset link missing from the text part");
        assert!(e.html.unwrap().contains(url), "reset link missing from html");
    }

    /// A silently-expired link is indistinguishable from a broken one.
    #[test]
    fn the_reset_states_its_expiry() {
        let e = password_reset("jo@example.com", "Jo", "https://x/y", 30, Lang::En);
        assert!(e.text.contains("30 minutes"));
        assert!(e.html.unwrap().contains("30 minutes"));
    }

    /// The whole reason this message exists.
    #[test]
    fn password_changed_tells_the_reader_what_to_do_if_it_was_not_them() {
        let e = password_changed("jo@example.com", "", "today", "https://x/help", Lang::En);
        assert!(e.text.to_lowercase().contains("did not change it"));
        assert!(e.html.unwrap().contains("https://x/help"));
    }

    /// Never "Hello ,".
    #[test]
    fn a_missing_name_falls_back_to_the_email_local_part() {
        let e = welcome("jo@example.com", "   ", "https://x", Lang::En);
        assert!(e.text.contains("jo"), "expected the local part: {}", e.text);
    }
}
