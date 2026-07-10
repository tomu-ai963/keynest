//! プリセットサービス定義(静的データ)。
//! 登録フォームの補完・.env スキャンのサービス推定・検証(Phase 4)の種データ。
//! scan.py の _ENV_VAR_SERVICES / _VALUE_PREFIX_SERVICES を統合したもの。

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Preset {
    /// サービス名(services.name に入る正規化済みの識別子)
    pub id: &'static str,
    pub display_name: &'static str,
    /// 推奨の環境変数名(フォームの初期値)
    pub env_var: &'static str,
    /// このサービスと推定できる環境変数名の別名
    pub env_var_aliases: &'static [&'static str],
    /// 値のプレフィックス(スキャン時のサービス推定に使用)
    pub value_prefixes: &'static [&'static str],
    /// キーを発行・管理するページの URL
    pub key_url: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "anthropic",
        display_name: "Anthropic",
        env_var: "ANTHROPIC_API_KEY",
        env_var_aliases: &["CLAUDE_API_KEY"],
        value_prefixes: &["sk-ant-"],
        key_url: "https://console.anthropic.com/settings/keys",
    },
    Preset {
        id: "openai",
        display_name: "OpenAI",
        env_var: "OPENAI_API_KEY",
        env_var_aliases: &[],
        value_prefixes: &["sk-proj-"],
        key_url: "https://platform.openai.com/api-keys",
    },
    Preset {
        id: "cloudflare",
        display_name: "Cloudflare",
        env_var: "CLOUDFLARE_API_TOKEN",
        env_var_aliases: &["CF_API_TOKEN", "CF_ACCOUNT_ID", "CLOUDFLARE_ACCOUNT_ID"],
        value_prefixes: &[],
        key_url: "https://dash.cloudflare.com/profile/api-tokens",
    },
    Preset {
        id: "stripe",
        display_name: "Stripe",
        env_var: "STRIPE_SECRET_KEY",
        env_var_aliases: &[
            "STRIPE_API_KEY",
            "STRIPE_PUBLISHABLE_KEY",
            "STRIPE_WEBHOOK_SECRET",
        ],
        value_prefixes: &[
            "sk_live_", "sk_test_", "pk_live_", "pk_test_", "whsec_", "rk_live_", "rk_test_",
        ],
        key_url: "https://dashboard.stripe.com/apikeys",
    },
    Preset {
        id: "resend",
        display_name: "Resend",
        env_var: "RESEND_API_KEY",
        env_var_aliases: &[],
        value_prefixes: &["re_"],
        key_url: "https://resend.com/api-keys",
    },
    Preset {
        id: "supabase",
        display_name: "Supabase",
        env_var: "SUPABASE_SERVICE_ROLE_KEY",
        env_var_aliases: &["SUPABASE_ANON_KEY", "SUPABASE_KEY", "SUPABASE_ACCESS_TOKEN"],
        value_prefixes: &["sbp_"],
        key_url: "https://supabase.com/dashboard/project/_/settings/api",
    },
    Preset {
        id: "github",
        display_name: "GitHub",
        env_var: "GITHUB_TOKEN",
        env_var_aliases: &["GH_TOKEN"],
        value_prefixes: &["ghp_", "github_pat_", "gho_", "ghs_"],
        key_url: "https://github.com/settings/tokens",
    },
    Preset {
        id: "google",
        display_name: "Google (Gemini / Cloud)",
        env_var: "GOOGLE_API_KEY",
        env_var_aliases: &["GEMINI_API_KEY"],
        value_prefixes: &["AIza"],
        key_url: "https://aistudio.google.com/apikey",
    },
    Preset {
        id: "aws",
        display_name: "AWS",
        env_var: "AWS_SECRET_ACCESS_KEY",
        env_var_aliases: &["AWS_ACCESS_KEY_ID", "AWS_SESSION_TOKEN"],
        value_prefixes: &[],
        key_url: "https://console.aws.amazon.com/iam/home#/security_credentials",
    },
    Preset {
        id: "azure",
        display_name: "Azure OpenAI",
        env_var: "AZURE_OPENAI_API_KEY",
        env_var_aliases: &["AZURE_API_KEY"],
        value_prefixes: &[],
        key_url: "https://portal.azure.com/",
    },
    Preset {
        id: "vercel",
        display_name: "Vercel",
        env_var: "VERCEL_TOKEN",
        env_var_aliases: &[],
        value_prefixes: &[],
        key_url: "https://vercel.com/account/tokens",
    },
    Preset {
        id: "netlify",
        display_name: "Netlify",
        env_var: "NETLIFY_AUTH_TOKEN",
        env_var_aliases: &[],
        value_prefixes: &["nfp_"],
        key_url: "https://app.netlify.com/user/applications",
    },
    Preset {
        id: "sendgrid",
        display_name: "SendGrid",
        env_var: "SENDGRID_API_KEY",
        env_var_aliases: &[],
        value_prefixes: &["SG."],
        key_url: "https://app.sendgrid.com/settings/api_keys",
    },
    Preset {
        id: "twilio",
        display_name: "Twilio",
        env_var: "TWILIO_AUTH_TOKEN",
        env_var_aliases: &["TWILIO_ACCOUNT_SID", "TWILIO_API_KEY"],
        value_prefixes: &[],
        key_url: "https://console.twilio.com/",
    },
    Preset {
        id: "slack",
        display_name: "Slack",
        env_var: "SLACK_BOT_TOKEN",
        env_var_aliases: &["SLACK_APP_TOKEN", "SLACK_WEBHOOK_URL"],
        value_prefixes: &["xoxb-", "xoxp-", "xapp-"],
        key_url: "https://api.slack.com/apps",
    },
    Preset {
        id: "discord",
        display_name: "Discord",
        env_var: "DISCORD_BOT_TOKEN",
        env_var_aliases: &["DISCORD_TOKEN"],
        value_prefixes: &[],
        key_url: "https://discord.com/developers/applications",
    },
    Preset {
        id: "notion",
        display_name: "Notion",
        env_var: "NOTION_API_KEY",
        env_var_aliases: &["NOTION_TOKEN"],
        value_prefixes: &["secret_", "ntn_"],
        key_url: "https://www.notion.so/my-integrations",
    },
    Preset {
        id: "linear",
        display_name: "Linear",
        env_var: "LINEAR_API_KEY",
        env_var_aliases: &[],
        value_prefixes: &["lin_api_"],
        key_url: "https://linear.app/settings/api",
    },
    Preset {
        id: "figma",
        display_name: "Figma",
        env_var: "FIGMA_ACCESS_TOKEN",
        env_var_aliases: &[],
        value_prefixes: &["figd_"],
        key_url: "https://www.figma.com/developers/api#access-tokens",
    },
    Preset {
        id: "huggingface",
        display_name: "Hugging Face",
        env_var: "HF_TOKEN",
        env_var_aliases: &["HUGGINGFACE_API_KEY", "HUGGING_FACE_HUB_TOKEN"],
        value_prefixes: &["hf_"],
        key_url: "https://huggingface.co/settings/tokens",
    },
    Preset {
        id: "replicate",
        display_name: "Replicate",
        env_var: "REPLICATE_API_TOKEN",
        env_var_aliases: &[],
        value_prefixes: &["r8_"],
        key_url: "https://replicate.com/account/api-tokens",
    },
    Preset {
        id: "groq",
        display_name: "Groq",
        env_var: "GROQ_API_KEY",
        env_var_aliases: &[],
        value_prefixes: &["gsk_"],
        key_url: "https://console.groq.com/keys",
    },
    Preset {
        id: "mistral",
        display_name: "Mistral AI",
        env_var: "MISTRAL_API_KEY",
        env_var_aliases: &[],
        value_prefixes: &[],
        key_url: "https://console.mistral.ai/api-keys/",
    },
    Preset {
        id: "cohere",
        display_name: "Cohere",
        env_var: "COHERE_API_KEY",
        env_var_aliases: &["CO_API_KEY"],
        value_prefixes: &[],
        key_url: "https://dashboard.cohere.com/api-keys",
    },
    Preset {
        id: "deepseek",
        display_name: "DeepSeek",
        env_var: "DEEPSEEK_API_KEY",
        env_var_aliases: &[],
        value_prefixes: &[],
        key_url: "https://platform.deepseek.com/api_keys",
    },
    Preset {
        id: "xai",
        display_name: "xAI (Grok)",
        env_var: "XAI_API_KEY",
        env_var_aliases: &["GROK_API_KEY"],
        value_prefixes: &["xai-"],
        key_url: "https://console.x.ai/",
    },
    Preset {
        id: "elevenlabs",
        display_name: "ElevenLabs",
        env_var: "ELEVENLABS_API_KEY",
        env_var_aliases: &["XI_API_KEY"],
        value_prefixes: &[],
        key_url: "https://elevenlabs.io/app/settings/api-keys",
    },
    Preset {
        id: "pinecone",
        display_name: "Pinecone",
        env_var: "PINECONE_API_KEY",
        env_var_aliases: &[],
        value_prefixes: &["pcsk_"],
        key_url: "https://app.pinecone.io/",
    },
    Preset {
        id: "tavily",
        display_name: "Tavily",
        env_var: "TAVILY_API_KEY",
        env_var_aliases: &[],
        value_prefixes: &["tvly-"],
        key_url: "https://app.tavily.com/",
    },
    Preset {
        id: "brave",
        display_name: "Brave Search",
        env_var: "BRAVE_API_KEY",
        env_var_aliases: &["BRAVE_SEARCH_API_KEY"],
        value_prefixes: &[],
        key_url: "https://api-dashboard.search.brave.com/app/keys",
    },
];

/// 値が公知のプレフィックス(sk-ant- 等)で始まる場合、そのプレフィックスを返す。
/// マスク表示の先頭に露出してよいのはこの範囲(=公開情報)だけ。
/// 複数該当する場合はより長い(具体的な)プレフィックスを優先する。
pub fn known_prefix(value: &str) -> Option<&'static str> {
    PRESETS
        .iter()
        .flat_map(|p| p.value_prefixes.iter().copied())
        .filter(|pre| value.starts_with(pre))
        .max_by_key(|pre| pre.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn known_prefix_matches_public_prefixes_only() {
        assert_eq!(known_prefix("sk-ant-api03-secret"), Some("sk-ant-"));
        assert_eq!(known_prefix("sk_live_abcdef"), Some("sk_live_"));
        assert_eq!(known_prefix("random-value-no-prefix"), None);
    }

    #[test]
    fn has_30_presets_with_unique_ids() {
        assert_eq!(PRESETS.len(), 30);
        let ids: HashSet<&str> = PRESETS.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), 30);
    }

    #[test]
    fn ids_are_normalized() {
        for preset in PRESETS {
            assert_eq!(
                preset.id,
                preset.id.trim().to_lowercase(),
                "id: {}",
                preset.id
            );
            assert!(!preset.env_var.is_empty());
        }
    }
}
