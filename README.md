# discord-send-notifier

軽量な単一バイナリの **Discord Webhook 通知 CLI**（Rust 製）。

Python スクリプトをインタプリタ起動して通知する代わりに、起動コストの小さいネイティブ単一バイナリで
Discord へ POST します。負荷監視のように「通知のために更に負荷を上げたくない」用途に向いています。

- 依存ランタイム不要（静的バイナリ・musl でリンク）
- `.env` / 環境変数 / 引数から Webhook URL を解決
- 単純メッセージも、色付き Embed（タイトル・説明・フィールド・footer・timestamp）も送れる
- 送信失敗時は終了コード 1（シェルパイプラインで検知可能）

## インストール

[Releases](https://github.com/densuke/discord-send-notifier-rs/releases) から
`discord-send-notifier-x86_64-linux-musl` をダウンロードして実行権を付けて配置します。

```bash
install -m 0755 discord-send-notifier-x86_64-linux-musl ~/.local/bin/discord-send-notifier
```

ソースからビルドする場合:

```bash
cargo build --release
# 静的 Linux バイナリ（他マシン配布向け）
cargo build --release --target x86_64-unknown-linux-musl
```

## 使い方

### Webhook URL の指定（解決順）

1. `--webhook-url <URL>`
2. 環境変数 `DISCORD_WEBHOOK_URL`
3. `.env` ファイル（`--env-file` 指定、無ければカレントの `./.env`）の `DISCORD_WEBHOOK_URL=` 行

### 例

```bash
# 単純メッセージ（content のみ）
discord-send-notifier "ビルドが完了しました"

# 色付き Embed（警告）＋フィールド＋タイムスタンプ
discord-send-notifier \
  --level warning \
  --title "ディスク残量警告" \
  --description "空き容量が閾値を下回りました" \
  --field "残量:2%:true" \
  --field "パス:/var:true" \
  --footer "🏢 ノード: shiten" \
  --timestamp

# メンション付き
discord-send-notifier --mention here "デプロイが失敗しました" --level error
```

### オプション

| オプション | 説明 |
|---|---|
| `[MESSAGE]` | 本文（content）。Embed 系オプションが無ければこれ単体で送る |
| `-l, --level <LEVEL>` | `info`（既定）/ `success` / `warning` / `error`。Embed の色を決める |
| `-t, --title <TITLE>` | Embed のタイトル |
| `-d, --description <DESC>` | Embed の説明（未指定なら MESSAGE を使う） |
| `-f, --field <Name:Value[:inline]>` | Embed フィールド。複数指定可。`inline` は `true`/`false`（既定 false） |
| `-m, --mention <here\|everyone\|ID>` | メンション |
| `--footer <TEXT>` | Embed の footer テキスト（発信元・ノード名など） |
| `--timestamp` | Embed に現在時刻（UTC）を含める |

### 全通知へ発信元を常時載せる（ドロップイン置換向け）

環境変数 **`DISCORD_DEFAULT_FOOTER`** を設定すると、`--footer` 未指定の全通知にその footer が
自動で付きます（単純メッセージでも最小 Embed を作って載せます）。既存の通知スクリプトを
「発信元 footer を常に付ける」挙動のまま本ツールへ差し替える用途に使えます。

```bash
export DISCORD_DEFAULT_FOOTER="🏢 ノード: shiten"
discord-send-notifier "定期バッチ完了"   # → footer 付きで届く
```

`--footer` を明示した場合はそちらが優先されます。
| `--webhook-url <URL>` | Webhook URL を直接指定 |
| `--env-file <PATH>` | Webhook URL を読む `.env` のパス |

## ライセンス

MIT License — [LICENSE](LICENSE) を参照。
