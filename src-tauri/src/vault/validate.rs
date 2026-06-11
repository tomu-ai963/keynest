//! サービス別のキー検証(validators.py の移植)。
//! 実際にプロバイダ API を叩いてキーの生死を確認する。
//!
//! 各バリデータは Validation { ok, detail } を返す。HTTP エラーや
//! タイムアウトは ok=false として detail に理由を入れる(Err にはしない)。
//! 検証未対応のサービスだけ Err(InvalidInput) を返す。

use std::time::Duration;

use serde::Serialize;

use crate::error::{Error, Result};

const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Serialize)]
pub struct Validation {
    pub ok: bool,
    pub detail: String,
}

type ValidatorFn = fn(&ureq::Agent, &str) -> Validation;

const VALIDATORS: &[(&str, ValidatorFn)] = &[
    ("anthropic", anthropic),
    ("cloudflare", cloudflare),
    ("github", github),
    ("openai", openai),
    ("resend", resend),
    ("stripe", stripe),
];

pub fn is_supported(service: &str) -> bool {
    VALIDATORS.iter().any(|(name, _)| *name == service)
}

pub fn validate(service: &str, value: &str) -> Result<Validation> {
    let service = service.trim().to_lowercase();
    let Some((_, validator)) = VALIDATORS.iter().find(|(name, _)| *name == service) else {
        let supported: Vec<&str> = VALIDATORS.iter().map(|(name, _)| *name).collect();
        return Err(Error::InvalidInput(format!(
            "検証未対応のサービスです: {service}(対応: {})",
            supported.join(", ")
        )));
    };
    let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
    Ok(validator(&agent, value))
}

/// ステータスコードだけで生死を判定する(200 = 有効)。
fn by_status(result: std::result::Result<ureq::Response, ureq::Error>) -> Validation {
    match result {
        Ok(r) => Validation {
            ok: r.status() == 200,
            detail: format!("HTTP {}", r.status()),
        },
        Err(ureq::Error::Status(code, _)) => Validation {
            ok: false,
            detail: format!("HTTP {code}"),
        },
        Err(e) => Validation {
            ok: false,
            detail: format!("リクエストエラー: {e}"),
        },
    }
}

fn anthropic(agent: &ureq::Agent, value: &str) -> Validation {
    by_status(
        agent
            .get("https://api.anthropic.com/v1/models")
            .set("x-api-key", value)
            .set("anthropic-version", "2023-06-01")
            .call(),
    )
}

fn cloudflare(agent: &ureq::Agent, value: &str) -> Validation {
    let result = agent
        .get("https://api.cloudflare.com/client/v4/user/tokens/verify")
        .set("Authorization", &format!("Bearer {value}"))
        .call();
    match result {
        // 200 でもボディの success フィールドまで確認する
        Ok(r) => {
            let status = r.status();
            let success = r
                .into_json::<serde_json::Value>()
                .ok()
                .and_then(|v| v.get("success").and_then(|s| s.as_bool()))
                .unwrap_or(false);
            Validation {
                ok: status == 200 && success,
                detail: format!("HTTP {status}"),
            }
        }
        Err(ureq::Error::Status(code, _)) => Validation {
            ok: false,
            detail: format!("HTTP {code}"),
        },
        Err(e) => Validation {
            ok: false,
            detail: format!("リクエストエラー: {e}"),
        },
    }
}

fn github(agent: &ureq::Agent, value: &str) -> Validation {
    by_status(
        agent
            .get("https://api.github.com/user")
            .set("Authorization", &format!("Bearer {value}"))
            .set("User-Agent", "keynest")
            .call(),
    )
}

fn openai(agent: &ureq::Agent, value: &str) -> Validation {
    by_status(
        agent
            .get("https://api.openai.com/v1/models")
            .set("Authorization", &format!("Bearer {value}"))
            .call(),
    )
}

fn resend(agent: &ureq::Agent, value: &str) -> Validation {
    by_status(
        agent
            .get("https://api.resend.com/domains")
            .set("Authorization", &format!("Bearer {value}"))
            .call(),
    )
}

fn stripe(agent: &ureq::Agent, value: &str) -> Validation {
    by_status(
        agent
            .get("https://api.stripe.com/v1/balance")
            .set("Authorization", &format!("Bearer {value}"))
            .call(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_services_match_validator_table() {
        assert!(is_supported("anthropic"));
        assert!(is_supported("stripe"));
        assert!(!is_supported("myapp"));
    }

    #[test]
    fn validate_rejects_unsupported_service() {
        let err = validate("myapp", "value").unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
        assert!(err.to_string().contains("anthropic"));
    }
}
