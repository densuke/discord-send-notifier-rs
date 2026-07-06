//! discord-send-notifier — 軽量な単一バイナリの Discord Webhook 通知 CLI。
//!
//! Python スクリプト（例: `send_notification.py`）をインタプリタ起動して通知する代わりに、
//! 起動コストの小さいネイティブ単一バイナリで Discord へ POST する。負荷の高い状況でも
//! 「通知のために更に負荷を上げる」ことを避けたい用途向け。
//!
//! Webhook URL の解決順:
//!   (1) `--webhook-url <URL>`
//!   (2) 環境変数 `DISCORD_WEBHOOK_URL`
//!   (3) `.env` ファイル（`--env-file` 指定、無ければカレントの `./.env`）の `DISCORD_WEBHOOK_URL=` 行
//!
//! 送信失敗時は stderr にエラーを出し、終了コード 1 で終わる（シェルパイプラインで検知可能）。

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::Parser;
use serde_json::{Value, json};

/// 軽量な Discord Webhook 通知 CLI。
///
/// 例:
///   discord-send-notifier "ビルドが完了しました"
///   discord-send-notifier -l warning -t "警告" -d "ディスク残量が少ない" --field "空き:2%:true" --timestamp
#[derive(Parser, Debug)]
#[command(name = "discord-send-notifier", version, about)]
struct Args {
    /// 本文メッセージ（content）。Embed 系オプションが無ければこれ単体で送る。
    message: Option<String>,

    /// 通知レベル（Embed の色を決める）。
    #[arg(short, long, default_value = "info", value_parser = ["info", "success", "warning", "error"])]
    level: String,

    /// Embed のタイトル。
    #[arg(short, long)]
    title: Option<String>,

    /// Embed の説明（未指定なら message を使う）。
    #[arg(short, long)]
    description: Option<String>,

    /// Embed のフィールド。`Name:Value[:inline]` 形式。複数指定可。
    /// inline は `true`/`false`（既定 false）。
    #[arg(short, long)]
    field: Vec<String>,

    /// メンション。`here` / `everyone` / 具体的な ID。
    #[arg(short, long)]
    mention: Option<String>,

    /// Embed の footer テキスト（例: 発信元やノード名）。
    #[arg(long)]
    footer: Option<String>,

    /// Embed に現在時刻（UTC）の timestamp を含める。
    #[arg(long)]
    timestamp: bool,

    /// Webhook URL を直接指定（環境変数・.env より優先）。
    #[arg(long)]
    webhook_url: Option<String>,

    /// Webhook URL を読む .env ファイルのパス（既定はカレントの ./.env）。
    #[arg(long)]
    env_file: Option<PathBuf>,
}

/// Discord のレベル→十進カラーコード。
fn level_color(level: &str) -> i64 {
    match level {
        "success" => 3066993,  // 緑
        "warning" => 15105570, // 黄
        "error" => 15158332,   // 赤
        _ => 3447003,          // info: 青
    }
}

/// メンション名を Discord 表記へ。`here`/`everyone` は特殊展開、それ以外はそのまま。
fn mention_str(m: &str) -> String {
    match m.to_lowercase().as_str() {
        "here" => "@here".to_string(),
        "everyone" => "@everyone".to_string(),
        other => other.to_string(),
    }
}

/// `KEY=VALUE` 行パーサ。`#` コメント/空行スキップ、前後 trim。最初の一致を返す。
fn parse_env_file(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') || !stripped.contains('=') {
            continue;
        }
        let (k, v) = stripped.split_once('=')?;
        if k.trim() == key {
            let value = v.trim();
            if value.is_empty() {
                return None;
            }
            return Some(value.to_string());
        }
    }
    None
}

/// Webhook URL を解決する（(1)引数 →(2)env →(3).env の順）。
fn resolve_webhook_url(args: &Args) -> Option<String> {
    // (1) --webhook-url。
    if let Some(u) = &args.webhook_url {
        let v = u.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    // (2) 環境変数。
    if let Ok(raw) = std::env::var("DISCORD_WEBHOOK_URL") {
        let v = raw.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    // (3) .env ファイル（--env-file 指定、無ければ ./.env）。
    let env_path = args
        .env_file
        .clone()
        .unwrap_or_else(|| PathBuf::from(".env"));
    if let Ok(text) = fs::read_to_string(&env_path)
        && let Some(v) = parse_env_file(&text, "DISCORD_WEBHOOK_URL")
    {
        return Some(v);
    }
    None
}

/// `Name:Value[:inline]` を Embed field の JSON へ。壊れた形式は None（呼び出し側で警告）。
fn parse_field(spec: &str) -> Option<Value> {
    // Python 版と同じく split(":", 2) 相当（最大3分割・3つ目に ':' を残す）。
    let mut it = spec.splitn(3, ':');
    let name = it.next()?.trim();
    let value = it.next()?.trim();
    if name.is_empty() {
        return None;
    }
    let inline = it
        .next()
        .map(|s| s.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    Some(json!({ "name": name, "value": value, "inline": inline }))
}

/// Unix エポック秒を UTC 民用時刻へ（Howard Hinnant の civil_from_days）。うるう秒は不問。
fn civil_from_epoch(epoch_secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = epoch_secs.div_euclid(86400);
    let secs_of_day = epoch_secs.rem_euclid(86400);
    let hour = (secs_of_day / 3600) as u32;
    let min = ((secs_of_day % 3600) / 60) as u32;
    let sec = (secs_of_day % 60) as u32;

    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d, hour, min, sec)
}

/// Embed 用 timestamp（RFC3339 UTC、末尾 Z）。
fn now_utc_rfc3339() -> String {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = civil_from_epoch(epoch);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// CLI 引数から送信ペイロード（`{"content":..,"embeds":[..]}`）を組み立てる。
/// Embed 系オプションが一切無ければ content 単体の単純通知にする。
fn build_payload(args: &Args) -> Value {
    let has_embed = args.title.is_some()
        || args.description.is_some()
        || !args.field.is_empty()
        || args.timestamp
        || args.footer.is_some()
        || args.level != "info";

    let mut payload = json!({});

    // content（message ＋ mention）。
    let mut content = args.message.clone();
    if let Some(m) = &args.mention {
        let ms = mention_str(m);
        content = Some(match content {
            Some(c) => format!("{ms} {c}"),
            None => ms,
        });
    }
    if let Some(c) = &content {
        payload["content"] = json!(c);
    }

    if has_embed {
        let mut embed = json!({ "color": level_color(&args.level) });
        if let Some(t) = &args.title {
            embed["title"] = json!(t);
        }
        // description は明示指定が無ければ message で補う。
        let desc = args.description.clone().or_else(|| args.message.clone());
        if let Some(d) = desc {
            embed["description"] = json!(d);
        }
        if args.timestamp {
            embed["timestamp"] = json!(now_utc_rfc3339());
        }
        if let Some(f) = &args.footer {
            embed["footer"] = json!({ "text": f });
        }
        let fields: Vec<Value> = args
            .field
            .iter()
            .filter_map(|spec| {
                let parsed = parse_field(spec);
                if parsed.is_none() {
                    eprintln!(
                        "WARN: フィールド形式が不正のためスキップ: '{spec}'（Name:Value[:true/false]）"
                    );
                }
                parsed
            })
            .collect();
        if !fields.is_empty() {
            embed["fields"] = json!(fields);
        }

        payload["embeds"] = json!([embed]);
    }

    payload
}

/// Discord へ送信する。リトライ最大3回。429 は Retry-After 秒 sleep、4xx(≠429) は即中断、
/// 5xx/ネットワークは `retry_delay * attempt` の backoff。成功(200/204)で Ok。
fn send_discord(webhook: &str, payload: &Value) -> Result<(), String> {
    const MAX_RETRIES: u32 = 3;
    let retry_delay = Duration::from_secs(2);
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build();
    let body = payload.to_string();

    for attempt in 1..=MAX_RETRIES {
        let resp = agent
            .post(webhook)
            .set("Content-Type", "application/json")
            .set(
                "User-Agent",
                concat!("discord-send-notifier-rs/", env!("CARGO_PKG_VERSION")),
            )
            .send_string(&body);

        match resp {
            Ok(r) => {
                let status = r.status();
                if status == 200 || status == 204 {
                    return Ok(());
                }
                // 想定外ステータス。ループ末尾の backoff で再送。
            }
            Err(ureq::Error::Status(code, r)) => {
                if code == 429 {
                    let retry_after = r
                        .header("Retry-After")
                        .and_then(|h| h.trim().parse::<u64>().ok())
                        .unwrap_or(2);
                    std::thread::sleep(Duration::from_secs(retry_after));
                    continue;
                } else if (400..500).contains(&code) {
                    return Err(format!("HTTP クライアントエラー: {code}"));
                }
                // 5xx はループ末尾の backoff で再送。
            }
            Err(ureq::Error::Transport(_)) => {
                // ネットワーク/タイムアウト。ループ末尾の backoff で再送。
            }
        }

        if attempt < MAX_RETRIES {
            std::thread::sleep(retry_delay * attempt);
        }
    }
    Err("最大リトライ回数を超えても送信できませんでした".to_string())
}

fn main() -> ExitCode {
    let args = Args::parse();

    // 送る中身が何も無い（message も embed 要素も無い）なら何もしない。
    if args.message.is_none()
        && args.title.is_none()
        && args.description.is_none()
        && args.field.is_empty()
        && !args.timestamp
        && args.footer.is_none()
        && args.mention.is_none()
        && args.level == "info"
    {
        eprintln!(
            "ERROR: 送信する内容がありません（message か Embed オプションを指定してください）"
        );
        return ExitCode::FAILURE;
    }

    let webhook = match resolve_webhook_url(&args) {
        Some(w) => w,
        None => {
            eprintln!(
                "ERROR: Webhook URL が未設定です（--webhook-url / 環境変数 DISCORD_WEBHOOK_URL / .env のいずれか）"
            );
            return ExitCode::FAILURE;
        }
    };

    let payload = build_payload(&args);
    match send_discord(&webhook, &payload) {
        Ok(()) => {
            println!("通知を送信しました。");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("ERROR: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_args() -> Args {
        Args {
            message: None,
            level: "info".to_string(),
            title: None,
            description: None,
            field: vec![],
            mention: None,
            footer: None,
            timestamp: false,
            webhook_url: None,
            env_file: None,
        }
    }

    #[test]
    fn colors_per_level() {
        assert_eq!(level_color("info"), 3447003);
        assert_eq!(level_color("success"), 3066993);
        assert_eq!(level_color("warning"), 15105570);
        assert_eq!(level_color("error"), 15158332);
        assert_eq!(level_color("unknown"), 3447003);
    }

    #[test]
    fn mention_expansion() {
        assert_eq!(mention_str("here"), "@here");
        assert_eq!(mention_str("EVERYONE"), "@everyone");
        assert_eq!(mention_str("123456"), "123456");
    }

    #[test]
    fn parse_env_file_basic() {
        let text = "# c\n\nDISCORD_WEBHOOK_URL=https://example/hook\n";
        assert_eq!(
            parse_env_file(text, "DISCORD_WEBHOOK_URL"),
            Some("https://example/hook".to_string())
        );
        assert_eq!(parse_env_file("KEY=\n", "KEY"), None);
        assert_eq!(parse_env_file("OTHER=1\n", "KEY"), None);
        assert_eq!(
            parse_env_file("  KEY  =  v  \n", "KEY"),
            Some("v".to_string())
        );
    }

    #[test]
    fn parse_field_variants() {
        assert_eq!(
            parse_field("空き:2%:true"),
            Some(json!({ "name": "空き", "value": "2%", "inline": true }))
        );
        assert_eq!(
            parse_field("Name:Value"),
            Some(json!({ "name": "Name", "value": "Value", "inline": false }))
        );
        // 3分割目に ':' を残す（URL 等）。
        assert_eq!(
            parse_field("URL:http://x:y"),
            Some(json!({ "name": "URL", "value": "http", "inline": false }))
        );
        // 名前空・値なしは None。
        assert_eq!(parse_field(":v:true"), None);
        assert_eq!(parse_field("only-name"), None);
    }

    #[test]
    fn simple_message_is_content_only() {
        let mut a = base_args();
        a.message = Some("hello".to_string());
        let p = build_payload(&a);
        assert_eq!(p["content"], "hello");
        assert!(
            p.get("embeds").is_none(),
            "embed should not exist for simple message"
        );
    }

    #[test]
    fn embed_built_when_title_present() {
        let mut a = base_args();
        a.title = Some("T".to_string());
        a.description = Some("D".to_string());
        a.level = "warning".to_string();
        let p = build_payload(&a);
        let embed = &p["embeds"][0];
        assert_eq!(embed["color"], 15105570);
        assert_eq!(embed["title"], "T");
        assert_eq!(embed["description"], "D");
    }

    #[test]
    fn embed_description_falls_back_to_message() {
        let mut a = base_args();
        a.title = Some("T".to_string());
        a.message = Some("body".to_string());
        let p = build_payload(&a);
        assert_eq!(p["embeds"][0]["description"], "body");
        // message は content にも載る。
        assert_eq!(p["content"], "body");
    }

    #[test]
    fn embed_has_fields_and_footer_and_timestamp() {
        let mut a = base_args();
        a.level = "success".to_string();
        a.field = vec!["k1:v1:true".to_string(), "k2:v2".to_string()];
        a.footer = Some("🏢 ノード: shiten".to_string());
        a.timestamp = true;
        let p = build_payload(&a);
        let embed = &p["embeds"][0];
        let fields = embed["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0]["inline"], true);
        assert_eq!(fields[1]["inline"], false);
        assert_eq!(embed["footer"]["text"], "🏢 ノード: shiten");
        assert!(embed["timestamp"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    fn mention_prepended_to_content() {
        let mut a = base_args();
        a.message = Some("deploy done".to_string());
        a.mention = Some("here".to_string());
        let p = build_payload(&a);
        assert_eq!(p["content"], "@here deploy done");
    }

    #[test]
    fn webhook_precedence_arg_over_env() {
        let mut a = base_args();
        a.webhook_url = Some("https://arg/hook".to_string());
        assert_eq!(
            resolve_webhook_url(&a),
            Some("https://arg/hook".to_string())
        );
    }

    #[test]
    fn webhook_from_env_file() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("dsn_env_{}.env", std::process::id()));
        fs::write(&path, "DISCORD_WEBHOOK_URL=https://file/hook\n").unwrap();
        let mut a = base_args();
        a.env_file = Some(path.clone());
        // env が設定されていると (2) が優先されるため、この経路検証は env 非設定前提。
        if std::env::var("DISCORD_WEBHOOK_URL").is_err() {
            assert_eq!(
                resolve_webhook_url(&a),
                Some("https://file/hook".to_string())
            );
        }
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn rfc3339_shape() {
        let s = now_utc_rfc3339();
        assert_eq!(s.len(), 20);
        assert!(s.ends_with('Z') && s.contains('T'));
    }

    #[test]
    fn civil_from_epoch_known() {
        assert_eq!(civil_from_epoch(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil_from_epoch(1609459200), (2021, 1, 1, 0, 0, 0));
    }
}
