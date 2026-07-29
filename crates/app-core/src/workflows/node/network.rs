use super::{req_text, FileVal, PortValue, PortValues};
use serde_json::Value;

/// POST form data to `url`. Fields are form fields; headers are extra request headers.
/// If a `file` is provided the request is multipart/form-data; otherwise form-urlencoded.
pub(super) fn run_http_post(_config: &Value, inputs: &PortValues) -> Result<PortValues, String> {
    let url = req_text(inputs, "url")?.to_string();
    let headers = inputs
        .get("headers")
        .and_then(PortValue::as_dict)
        .unwrap_or(&[]);
    let fields = inputs
        .get("fields")
        .and_then(PortValue::as_dict)
        .unwrap_or(&[]);
    let file = inputs.get("file").and_then(PortValue::as_file);

    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let mut req = agent.post(&url);
    for (k, v) in headers {
        req = req.set(k, v);
    }

    let result = if let Some(f) = file {
        let boundary = "----sentinelFormBoundary7MA4YWxkTrZu0gW";
        let body = multipart_body(fields, f, boundary);
        req.set(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send_bytes(&body)
    } else {
        let form: Vec<(&str, &str)> = fields
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        req.send_form(&form)
    };

    let (status, body, ok) = match result {
        Ok(resp) => {
            let s = resp.status();
            (
                s as i64,
                resp.into_string().unwrap_or_default(),
                (200..300).contains(&s),
            )
        }
        Err(ureq::Error::Status(code, resp)) => {
            (code as i64, resp.into_string().unwrap_or_default(), false)
        }
        Err(e) => return Err(format!("HTTP POST to {url} failed: {e}")),
    };

    let mut o = PortValues::new();
    o.insert("status".into(), PortValue::Num(status));
    o.insert("response".into(), PortValue::Text(body));
    o.insert("ok".into(), PortValue::Bool(ok));
    Ok(o)
}

fn multipart_body(fields: &[(String, String)], file: &FileVal, boundary: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(file.bytes.len() + 1024);
    for (k, v) in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{k}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(v.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n",
            file.name
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", file.mime).as_bytes());
    body.extend_from_slice(&file.bytes);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}
