# deft-reversi-desktop アーキテクチャ・実装計画

作成日: 2026-11-01。対象ブランチ: `feature/gui`。調査時のエンジン: `f13f68a`。

## 1. 確定仕様と本計画での提案

### 確定仕様

| 項目 | 決定内容 |
|---|---|
| 対象 | 一般ユーザー向けオセロアプリ |
| 対応OS | Windows / Linux |
| GUI | GPUI |
| 開発言語 | Rust |
| AI | deft-reversi（`deft_reversi_engine`） |
| 性能方針 | 高速起動・低遅延・探索性能への影響を最小化 |
| 動作形態 | ネイティブデスクトップアプリ |
| 機能 | 人間 vs AI、ローカル2人対局、AI vs AI、合法手の表示、Undo / Redo、棋譜保存・読込、局面編集、難易度設定、思考時間設定、解析モード、石の反転アニメーション、ダーク / ライトモード、効果音 |
| 棋譜保存 | 自動保存＋エクスポート |
| 不採用 | オンライン対局 |

### 本計画での提案（実装の入口で使う暫定仕様）

以下は計画上の提案で、実測済みの保証ではない。P0〜P1の結果で更新する。

- 対応アーキテクチャは両OSのx86_64。検証基準はWindows 11、Ubuntu 24.04以降。LinuxはWayland / X11の両方を確認する。
- 思考時間は「AIの1手あたりの探索上限」。対局時計・秒読みは初回範囲外。
- 棋譜はバージョン付き独自JSON（拡張子`.dreversi`）。読込・エクスポートは同形式を完成させ、他形式は後続。
- 解析は現在の局面が対象。常時バックグラウンド解析・全棋譜一括解析は後続。
- 初期値はプリセット「中級」・合法手表示ON・テーマOS追従・効果音ON。
- 棋譜の分岐ツリー、定石の同梱、ARM64、自動更新は初回範囲外（§14）。

## 2. 既存資産と制約

| 対象 | 事実 | 設計への影響 |
|---|---|---|
| workspace | メンバーは`deft-reversi-engine`と`deft-reversi-book`。旧CLI / Web / learnは除外 | `deft-reversi-desktop`をmembersに追加する |
| `Position` | 盤面＋手番。`legal_moves`・`flips`・`play`・`must_pass`・`is_terminal`・`final_score`・`from_colors`・文字列入出力 | ルールをGUIで再実装しない。履歴は`Position`の列で持つ |
| `EngineWorker::start` | 即時復帰。探索ごとに8 MiBスタックのスレッドを生成。探索中は`EngineError::Busy` | 同時探索は1件。サービス側で直列化する |
| `SearchHandle` | `events()`はstd mpsc、`stop()`、`is_finished()`、`wait()`。Dropは停止要求のみで完了を待たない | `wait`と`EngineWorker::with`はUIスレッドで呼ばない |
| `SearchRequest` | `Goal::Level(1..=60)`＋`time_limit`を併用可。`Level`と`selectivity`の併用はエラー | 要求ごとに新しいrequestを作る |
| `SearchResult` | `score`は探索開始局面の手番側の石差。`None`は完了反復なし。`is_solved()`で完全読みを判別。途中停止でも`best_move`は合法手 | 表示は黒視点へ変換。未評価を0点と扱わない |
| `all_moves()` | 最善手探索に加え全候補を再探索するため数倍遅い。時間切れでは候補が空になりうる | 解析の明示操作に限定する |
| 評価モデル | `Model::from_file` / `from_bytes`はDPR2（版1・2）だけを読む。版2は重みとProbCut校正を1ファイルに持つ。モデルはGitに入れず、GitHub Release（`model-*`タグ）で配る。現行は`model-v2-pattern-lr0.3`の`deft-eval-v2-pattern-lr0.3.dpr2`（35,842,568 bytes） | 同梱するモデルはこのリリース資産。読込とTT確保は最初の描画から切り離す |
| 評価の段階 | 段階は`60 - empty_count`を0〜59に丸めて選ぶ | 石4個未満の編集局面も評価自体は可能。ただし学習範囲外なので§9で扱う |
| ビルド設定 | release: `lto = "fat"`、`codegen-units = 1`、`panic = 'unwind'`。toolchain 1.99.1 | `panic = 'unwind'`は並列探索の後処理に必要なので維持する。配布版に`target-cpu=native`を付けない |

根拠: [API設計](../deft-reversi-engine/docs/api.md)、[worker実装](../deft-reversi-engine/src/worker.rs)、[探索API](../deft-reversi-engine/src/search/api.rs)、[旧アプリの扱い](legacy-applications.md)。

## 3. 全体アーキテクチャ

### 3.1 層と依存方向

```text
┌──────────── ui（GPUI依存） ────────────┐
│ RootView / BoardView / SidePanel / Dialogs │
│   Actions・キー入力 → Command             │
│   AppModel（Entity）を observe して描画   │
└───────────────┬────────────────────────┘
                │ Command ↓        ↑ 状態変化の通知（cx.notify）
┌───────────────▼────────────────────────┐
│ app::Controller（GPUI非依存の状態機械）   │
│   apply(Command) -> Vec<Effect>          │
│   on_engine(EngineEvent) / on_storage(..) │
└──┬───────────┬───────────┬──────────────┘
   │           │           │  Effect（探索要求・保存要求・効果音）
┌──▼───────┐ ┌─▼─────────┐ ┌▼──────────┐
│ domain   │ │ services  │ │persistence│
│ Session  │ │ Engine    │ │ record DTO│
│ History  │ │ Storage   │ │ paths     │
│ Editor   │ │ Audio     │ │ atomic I/O│
│ Settings │ └─┬─────────┘ └───────────┘
└──┬───────┘   │
   └─────┬─────┘
         ▼
 deft_reversi_engine（Position / Action / EngineWorker）
```

- `domain`・`app`・`persistence`はGPUIに依存しない。Controllerは「Commandを受けて状態を変え、副作用をEffectとして返す」純粋に近い形にし、ヘッドレス試験の対象にする。
- `services`は副作用の実行者。Controllerはサービスをtraitで受け取り、試験ではfakeに差し替える。
- `ui`はControllerの状態を読むだけで、ルール判定や保存形式を持たない。
- エンジンへGPUIや保存形式の依存を持ち込まない。エンジン型に`serde`を追加せず、保存用DTOへ変換する。

### 3.2 スレッド構成

| スレッド | 所有するもの | ブロックしてよいか |
|---|---|---|
| GPUIメインスレッド | AppModel（Controller）、View、アニメーション | しない。I/O・`wait`・`with`を呼ばない |
| `engine-ctl`（1本） | `EngineWorker`、実行中の`SearchHandle`、保留要求 | する（コマンド受信・`wait`・進捗受信） |
| `deft-search`（探索ごと） | 探索本体。エンジン内部の並列ワーカーを使う | 既存実装のまま |
| `storage`（1本） | 書込みキュー、debounce、ファイルロック | する |
| 音声 | 出力デバイス（`rodio`のストリーム） | UIからは非同期に再生要求だけ送る |

UIへの戻りは`async-channel`（または`futures::channel::mpsc`）で受け、GPUIの`cx.spawn`内のループで`recv().await`する。サービス側は`send_blocking`で送る。これで待機中の周期ポーリングをなくす。

### 3.3 クレート構成

最初は1クレート内のモジュールで責務を分ける。再利用先（例: CUIの新版）ができた時点で`domain`と`persistence`を別クレートへ切り出す。

```text
deft-reversi-desktop/
  Cargo.toml
  build.rs                  # Windowsのアイコン・バージョン情報（winres等、P7）
  assets/
    sounds/                 # place.ogg / pass.ogg / gameover.ogg
    icons/
  src/
    main.rs                 # 起動順序、ログ、パニックフック、単一起動ロック
    app/
      mod.rs                # Controller、Command / Effect / Notice
      autoplay.rs           # 手番と対局状態から次の探索要求を決める
    domain/
      player.rs             # Player / AiSettings / Preset
      history.rs            # Position列・Action列・cursor・操作グループ
      session.rs            # GameSession、モード、終局判定
      editor.rs             # 局面編集ドラフト
      settings.rs           # 表示・音・エンジン資源の設定
    services/
      engine.rs             # EngineService（engine-ctlスレッド）
      storage.rs            # StorageService（storageスレッド）
      audio.rs              # AudioService
    persistence/
      record.rs             # 棋譜DTO v1・検証・移行
      settings_file.rs      # 設定DTO
      paths.rs              # OS別保存先
      atomic_file.rs        # 一時ファイル→置換、バックアップ
    ui/
      root.rs               # RootView、レイアウト、Action登録
      board.rs              # BoardView（入力・ハイライト）
      disc_anim.rs          # 反転アニメーション
      game_panel.rs         # 手番・石数・対局操作
      history_panel.rs      # 棋譜リストと移動
      analysis_panel.rs     # 評価・PV・候補手
      dialogs/              # 新規対局・設定・対局一覧・編集
      theme.rs              # semantic token、OS追従
  tests/
    controller.rs           # Command列→状態の試験（fake engine / storage）
    record_roundtrip.rs
    engine_service.rs       # 実エンジンの短時間探索・停止
```

### 3.4 依存クレート候補

| 用途 | 候補 | 備考 |
|---|---|---|
| GUI | `gpui`、`gpui_platform` | 同一のGit revisionに固定し、Cargo.lockを管理する |
| エンジン | `deft_reversi_engine`（path） | featuresなし |
| 保存 | `serde`、`serde_json` | エンジンは既に依存しているので追加コストは小さい |
| 保存先 | `directories` | OS標準ディレクトリ |
| 単一起動 | `fs4`等のファイルロック | データ領域を1プロセスで扱う |
| UIへの通知 | `async-channel` | GPUIのexecutorで`await`できる |
| 音 | `rodio`（ogg / wavのデコード機能のみ） | 起動時にデコード済みバッファを用意 |
| ファイルダイアログ | GPUIの`prompt_for_paths` / `prompt_for_new_path` | 足りなければ`rfd`を検討 |
| ログ | `tracing`、`tracing-subscriber` | ローテーション付きファイル出力 |
| 時刻 | `time`（UTC・RFC 3339） | 棋譜のタイムスタンプ |

Tokioはアプリ全体のためには追加しない。UI部品ライブラリも最初は必須にしない。版とfeaturesはP0で両OSのビルドを通してから固定する。

## 4. 主要な型とインターフェース

以下は設計の骨子であり、名前と欄はP1で調整する。

### 4.1 対局ドメイン

```rust
pub enum Player {
    Human,
    Ai(AiSettings),
}

pub struct AiSettings {
    pub level: u8,               // 1..=60
    pub time_limit: Option<Duration>, // None はレベルのみ
}

pub struct History {
    positions: Vec<Position>,    // positions.len() == actions.len() + 1
    actions: Vec<Action>,        // パスも Action::Pass として記録
    groups: Vec<usize>,          // 操作グループの開始インデックス（Undo単位）
    cursor: usize,               // 現在表示中の局面 = positions[cursor]
}

pub enum SessionStatus { Playing, Paused, Finished(Outcome) }
pub enum WorkspaceMode { Play, Analyze, Edit(EditorDraft) }

pub struct GameSession {
    pub id: GameId,              // UUID v4 または時刻ベースの一意ID
    pub players: [Player; 2],    // [黒, 白]
    pub history: History,
    pub status: SessionStatus,
    pub revision: u64,           // 局面・設定が変わるたびに単調増加
}
```

不変条件（試験で検査する）:

- `positions[i + 1] == positions[i].play(actions[i])`。
- `cursor <= actions.len()`。`cursor`より先はRedo対象。
- 終局でない局面で合法手がゼロならパスが自動で記録される。終局局面の後ろにActionはない。
- `revision`は巻き戻らない。Undoで同じ局面に戻っても新しい値になる。

### 4.2 Controller

```rust
pub enum Command {
    NewGame(NewGameOptions),
    Play(Square),
    Undo, Redo, GoTo(usize),
    Resume, Pause, StepAi, MoveNow,
    SetMode(WorkspaceMode),
    Analyze { all_moves: bool },
    EditorApply(EditorOp), EditorCommit, EditorCancel,
    Load(PathBuf), Export(PathBuf), OpenSaved(GameId),
    UpdateSettings(SettingsPatch),
    Quit,
}

pub enum Effect {
    StartSearch(SearchJob),
    CancelSearch,
    Save(SaveRequest),
    PlaySound(Sound),
    StartAnimation(FlipAnimation),
    ShowNotice(Notice),
}

impl Controller {
    pub fn apply(&mut self, command: Command) -> Vec<Effect>;
    pub fn on_engine(&mut self, event: EngineEvent) -> Vec<Effect>;
    pub fn on_storage(&mut self, event: StorageEvent) -> Vec<Effect>;
}
```

AppModel（GPUIのEntity）は`apply`の戻り値をサービスへ配送し、`cx.notify()`で再描画を要求する。Controller自体はGPUIの型に触れない。

### 4.3 EngineService

```rust
pub struct SearchJob {
    pub id: SearchId,                // 単調増加
    pub revision: u64,               // 要求時の GameSession::revision
    pub purpose: Purpose,            // Move { color } | Analysis
    pub position: Position,
    pub request: SearchSpec,         // goal・time_limit・all_moves（SearchRequestの素材）
}

pub enum EngineCommand {
    Start(SearchJob),                // 実行中があれば停止→回収→最新の保留1件のみ開始
    Cancel,
    Reconfigure(EngineResources),    // threads・tt_mib。回収後にEngineを再構築
    ClearTt,                         // 新規対局時
    Shutdown,
}

pub enum EngineEvent {
    Ready { metadata: ModelInfo, load_time: Duration },
    LoadFailed(String),
    Progress { id: SearchId, latest: ProgressView },   // 集約済み
    Finished { id: SearchId, result: SearchResult },
    Failed { id: SearchId, error: String },
}
```

`engine-ctl`の処理:

1. 起動直後にモデル読込と`Engine::new`を行い`Ready`か`LoadFailed`を送る。その間に来た`Start`は保留し、最新1件だけ残す。
2. `Start`を受けたら`SearchRequest`を新規に組み立てて`worker.start`する。
3. 探索中は`events().recv_timeout(5〜10 ms)`とコマンド受信を交互に行う。進捗は最新値だけ保持し、UIへは最大15回/秒に間引いて送る。`Finished` / `Failed`は間引かない。
4. `Cancel`・新しい`Start`・`Reconfigure`・`Shutdown`では`stop()`後に`wait()`で回収してから次へ進む。`Busy`の再試行ループは作らない。
5. 待機中はコマンドチャネルでブロックし、CPUを使わない。

Controllerは`Finished`を`id`・`revision`・`purpose`・`status`で照合し、着手用途ではさらに現在手番と合法性を再確認してから`Command::Play`相当の処理を行う。照合に失敗した結果は捨てる。

### 4.4 StorageService

```rust
pub enum StorageCommand {
    SaveGame { id: GameId, revision: u64, record: RecordV1 },  // debounce対象
    FlushNow,                                                  // 新規対局前・終了時
    SaveSettings(SettingsV1),
    Export { path: PathBuf, record: RecordV1 },
    Load { path: PathBuf },
    ListGames,
}

pub enum StorageEvent {
    Saved { id: GameId, revision: u64 },
    SaveFailed { id: GameId, error: String },
    Loaded(Result<RecordV1, LoadError>),
    Games(Vec<GameSummary>),
}
```

### 4.5 AudioService

起動時に3種類の音をデコードして保持する。`play(Sound)`は即時復帰。出力デバイスがない・失敗した場合は警告ログだけ残して無音で続ける。ミュートと音量は設定から反映する。

## 5. GPUIとの結合

### 5.1 Entityとビュー

- `AppModel`（`Entity<AppModel>`）: Controller、各サービスの送信側、アニメーション状態、表示専用状態（ホバー中のマス、開いているダイアログ）。
- `RootView`: `AppModel`を`observe`し、盤面・サイドパネル・ツールバーを構成する。
- `BoardView`: 盤面は正方形を保って中央に置く。最初は64マスのdiv要素で描画し、プロファイルで負荷が問題になったらカスタム`Element`（`paint_quad`等での直接描画）へ切り替える。
- ダイアログ: 新規対局、設定、対局一覧（自動保存の一覧）、局面編集。

### 5.2 サービスからの通知

```rust
cx.spawn(async move |this, cx| {
    while let Ok(event) = engine_events.recv().await {
        this.update(cx, |model, cx| {
            let effects = model.controller.on_engine(event);
            model.dispatch(effects, cx);
            cx.notify();
        })?;
    }
    anyhow::Ok(())
}).detach();
```

ストレージも同じ形にする。GPUIのAPIは版で変わるため、P0で固定したrevisionの書き方に合わせる。

### 5.3 アクションとキー操作

| アクション | Windows / Linux | 備考 |
|---|---|---|
| 新規対局 | Ctrl+N | ダイアログで対局モード・各色・プリセット |
| 棋譜を開く | Ctrl+O | 読込後は一時停止状態 |
| エクスポート | Ctrl+Shift+S | 自動保存があるのでCtrl+Sは「今すぐ保存」 |
| Undo / Redo | Ctrl+Z / Ctrl+Y・Ctrl+Shift+Z | §6の操作グループ単位 |
| 1手戻る / 進む | ←・→（棋譜パネル） | 1 Action単位 |
| 最初 / 最後 | Home・End | |
| 再開 / 一時停止 | Space | AI vs AIと人間 vs AI |
| 今すぐ指す | Ctrl+Enter | 最後の完了反復の手を採用 |
| 解析モード | F2 | Ctrl+Aは全選択の慣習と衝突するので使わない |
| 局面編集 | F3 | Escでキャンセル |
| 盤面フォーカス移動・着手 | 矢印・Enter | 盤面にフォーカスがあるとき |

## 6. 対局進行とUndo / Redo

### 6.1 3つの対局モード

対局モードは`players: [Player; 2]`の組合せだけで表す。

| モード | 黒 | 白 | 自動進行 |
|---|---|---|---|
| 人間 vs AI | Human / Ai | Ai / Human | AIの手番で自動探索 |
| ローカル2人 | Human | Human | なし（AIの準備に依存しない） |
| AI vs AI | Ai | Ai | 開始・一時停止・再開・1手進行。色ごとに設定を持つ |

`autoplay.rs`は「`status == Playing`かつ`mode == Play`かつ手番がAIかつ実行中の着手探索がない」ときに`StartSearch`を返す。この1か所以外から着手探索を始めない。

### 6.2 着手の流れ

1. `Command::Play(sq)`（人間）またはAIの`Finished`を受ける。
2. `Position::play`で検証し、`flips(sq)`で反転石を取得する。
3. cursorより先の履歴を破棄し、Actionを追加、`revision`を進める。
4. 次局面が終局でなく合法手ゼロならパスを自動追加し通知する。両者打てなければ`Finished(Outcome)`。
5. Effectとして、アニメーション、効果音、保存要求、（必要なら）次の探索要求を返す。探索はアニメーションの完了を待たない。

石数表示は盤上の実石数、終局時の結果は`final_score`（空きマスを勝者に加算）で、画面にはどちらの数え方かを明示する。

### 6.3 Undo / Redoの単位

- 内部の最小単位は1 Action（パスを含む）。
- 人間 vs AIのUndoボタンは「直前の人間の着手の前へ戻る」。AIの応手と自動パスを同じ操作グループに含める。Redoも同じグループを復元する。
- 人間の直前着手がない場合（初手がAIなど）は1 Action単位に戻す。
- 棋譜パネルの←→は常に1 Action単位。
- 履歴移動中は自動進行を一時停止（`Paused`）し、実行中の探索はキャンセルする。Redoでは探索を始めない。「再開」で現在局面から続ける。
- 履歴の途中で新しい手を打つと、その先のRedo履歴を破棄する（初回は分岐ツリーを持たない）。

### 6.4 AI vs AI

- 開始・一時停止・再開・1手進行を提供する。一時停止は実行中の探索をキャンセルし、その結果は採用しない。
- 「今すぐ指す」は停止要求後に返る最後の完了反復の手を採用する別コマンド。
- 着手と着手の間に最低表示時間（演出時間、既定250 ms）を置き、短時間の思考でも盤面の変化を追えるようにする。演出OFFなら置かない。

## 7. AI・難易度・思考時間・解析

### 7.1 難易度プリセット（暫定）

一般ユーザーには名称付きプリセットを見せ、詳細設定でレベル1〜60と思考時間を個別に選べるようにする。数値は対局試験で調整する前提の暫定値であり、棋力や段級位との対応は保証しない。

| プリセット | Level | 1手の上限時間 |
|---|---|---|
| 入門 | 1 | 0.5 s |
| 初級 | 4 | 0.5 s |
| 中級（既定） | 8 | 1 s |
| 上級 | 14 | 2 s |
| 達人 | 20 | 5 s |
| 最強 | 30 | 10 s |

- 要求は`SearchRequest::new(Goal::Level(level)).time_limit(limit)`。レベル目標に届けば早く指し、時間に達したら最後に得た合法手を使う。
- 思考時間の選択肢は0.5 / 1 / 2 / 5 / 10 / 30 s と「レベルのみ（上限なし）」。上限なしは詳細設定でのみ選べる。
- 時間は厳密な締切ではないため、停止要求から回収までの超過を計測する（§11）。
- 初心者向けのランダム性（わざと悪手を混ぜる等）はエンジンに機能がないため初回範囲外。入門の手応えが強すぎる場合はP2の試験で対応を決める。

### 7.2 資源設定

- スレッド既定値は`max(1, min(8, available_parallelism - 1))`、TTは128 MiB（エンジン既定は256 MiB）。設定で変更可能。TT確保に失敗したら半分で再試行する。
- 新規対局では`ClearTt`を送る。同じ対局中は毎手消去しない。
- 資源設定の変更は探索の停止・回収後にEngineを作り直して反映する。

### 7.3 解析モード

- 解析モードへ入ると自動進行を一時停止し、現在の局面（cursorの局面）を解析する。棋譜を移動すると古い解析をキャンセルし、新しい局面で要求し直す。
- 解析の既定要求は`Goal::Level(解析レベル)`＋上限時間（既定5 s）。「完全読み」ボタンで`Goal::Endgame`（`Selectivity::Full`）を要求する。
- 評価値は黒視点を標準とし、探索開始局面が白番なら符号を反転する。設定で手番視点に切り替えられる。
- 表示: 評価値、最善手、PV、探索種別（深さ・選択率 / 選択的終盤 / 完全読み）、ノード数、経過時間、停止理由。NPSは直近の進捗から算出し、進捗のない期間に値を補わない。
- `is_solved()`が真のときだけ「完全読み」と表示し、他は「推定」または「選択的探索」とする。`score == None`は「未評価」。勝率への変換はしない。
- 全候補評価は「候補手を評価」の明示操作で`all_moves()`を要求する。結果は盤上に数値で重ねる。中断で候補が空なら「候補評価未完了」と表示し、0点や完全な一覧として扱わない。
- 着手探索と解析はworkerを共有し、同時に走らせない。解析モード中にAIの手番が来ても自動では指さない。

### 7.4 定石

既存の`deft-reversi-book`は確定機能に含まれないため初回の必須経路にしない。追加する場合は`BookPlayer`で参照し、使える手がなければ通常探索へ戻す。初心者向けプリセットで強い定石を無条件に使わない。

## 8. 棋譜・設定・自動保存

### 8.1 棋譜形式 v1（`.dreversi`、UTF-8 JSON）

```json
{
  "format": "deft-reversi-record",
  "schema_version": 1,
  "game_id": "01J...",
  "created_at": "2026-11-01T09:00:00Z",
  "updated_at": "2026-11-01T09:12:34Z",
  "initial_position": "---------------------------OX------XO--------------------------- X",
  "actions": ["f5", "d6", "c3", "pass"],
  "cursor": 4,
  "action_groups": [0, 2, 3],
  "players": {
    "black": { "kind": "human" },
    "white": { "kind": "ai", "level": 8, "time_limit_ms": 1000 }
  },
  "status": "playing",
  "outcome": null,
  "engine": { "version": "2.0.0", "model": "<モデル識別情報>" }
}
```

- 正本は`initial_position`と`actions`。`outcome`・`status`は読込時に全手順を再生して再検証する。不一致ならファイル側を信用せず再計算値を使い、警告を出す。
- 読込時の検査: ファイルサイズ上限（1 MiB）、`format`、`schema_version`（未知の将来版は読まずにエラー）、手数上限、全手の合法性、`cursor`と`action_groups`の範囲。失敗したら開いている対局はそのまま維持する。
- 進捗・解析結果・GPU情報は保存しない。
- 読込・復旧の直後は必ず`Paused`にし、AIが勝手に続きを指さないようにする。

### 8.2 保存先

| 用途 | Windows | Linux |
|---|---|---|
| 設定 | `%APPDATA%\deft-reversi\settings.json` | `$XDG_CONFIG_HOME/deft-reversi/settings.json` |
| 自動保存の対局 | `%LOCALAPPDATA%\deft-reversi\games\<game_id>.dreversi` | `$XDG_DATA_HOME/deft-reversi/games/` |
| ログ・ロック | `%LOCALAPPDATA%\deft-reversi\logs\` | `$XDG_STATE_HOME/deft-reversi/` |

パスは`directories`で解決する。実行ファイルの隣には書かない。エクスポートだけ保存先をユーザーが選ぶ。

### 8.3 自動保存

- 契機: 着手の確定、履歴移動、対局設定の変更、編集の確定、対局終了。探索の進捗やアニメーションでは保存しない。
- debounceは200 ms、連続更新でも最長1秒で書き出す。新規対局・対局切替・終了の前は`FlushNow`で直前の対局を必ず書く。
- 書込みは`storage`スレッドが直列に行う。`(game_id, revision)`より古い要求は捨て、古い内容による上書きを防ぐ。
- 原子的な置換: 同じディレクトリの一時ファイルに書き、flush・`sync_all`の後に置換する。直前の正常版を`.bak`に残す。Windowsの置換（既存ファイルへのrename、ウイルス対策ソフトによる一時ロック）は実機で試験し、数回の短い再試行を入れる。
- `Saved`を受けた`revision`だけを保存済みとして扱う。失敗時は画面に通知し、再試行と別名エクスポートを提示する（ディスク満杯・権限不足）。
- 起動時は最後に開いていた対局を一時停止状態で復元する。破損時は`.bak`を試し、それも駄目なら元ファイルを`*.corrupt`として残し、新規対局で起動する。
- 対局一覧ダイアログで過去の対局（終局済みを含む）を開く・削除する（削除はOSのごみ箱ではなくアプリ内の確認つき削除）。保持件数の上限は設けず、一覧は更新日時順。
- 単一起動: データ領域のロックファイルで2つ目のプロセスを検出し、既存ウィンドウがあることを伝えて終了する。

### 8.4 設定

`settings.json`（`schema_version`付き）に、プリセット・詳細なAI設定、資源設定、合法手表示、評価の視点、テーマ、アニメーション（通常 / 短縮 / OFF）、効果音（ON/OFF・音量）、ウィンドウサイズを保存する。未知の欄は無視し、壊れていたら既定値で起動して元ファイルを退避する。

## 9. 局面編集と表示体験

### 9.1 局面編集

- 編集は現在の対局から独立した`EditorDraft`（黒・白のビットボードと手番）で行う。
- 操作: クリックで黒→白→空きの循環、パレットで配置色を選んでドラッグ塗り、手番指定、初期配置、全消去、局面文字列のコピー・貼付（エンジンの64マス＋手番形式）。
- 確定時は`Position::from_colors`で検証し、編集局面を開始局面とする新しい対局を作る。元の対局は保存済みの状態で一覧に残る。キャンセルなら元の対局へそのまま戻る。
- 手番側に合法手がなく相手にある局面はパスを補って開始し、双方なければ終局局面として表示・保存だけ許す。
- 石が4個未満の局面は評価関数の学習範囲外なので、初回は表示・編集・保存に限定し、AI対局と解析を無効にして理由を表示する。4個以上の極端な局面はP5で探索の健全性を確かめてから許可する。

### 9.2 画面構成

```text
┌──────────────────────────────────────────────────────┐
│ [新規] [開く] [エクスポート] [編集] [解析]   [設定]   │
├───────────────────────────────┬──────────────────────┤
│                               │ 黒 ● 人間      12     │
│                               │ 白 ○ AI 中級   20     │
│         盤面（正方形）         │ 手番: 黒 / AI思考中… │
│     合法手・直前手・候補値      │ [Undo][Redo][一時停止]│
│                               ├──────────────────────┤
│                               │ 棋譜  1. f5 d6 …      │
│                               ├──────────────────────┤
│                               │ 解析  +4  PV: c3 d3 … │
└───────────────────────────────┴──────────────────────┘
```

- 最低ウィンドウサイズはP0の試作で決める（目安 800×560）。幅が足りないときはサイドパネルを盤面の下に回す。
- DPI変更・リサイズでも盤面を正方形に保ち、石は円の描画で表す（大きな画像を必須にしない）。
- 座標ラベル、合法手、直前手、解析候補はテーマのsemantic tokenで色分けする。合法手は色だけでなく形（小さな点）でも区別する。
- キーボードで盤面を操作でき、フォーカス表示を出す。GPUIのアクセシビリティ対応の範囲で、マス名・石色・手番を読み上げに渡す。

### 9.3 反転アニメーション

- 確定した局面の差分（置いた石と`flips`の結果）から表示用の`FlipAnimation`を作る。対局状態には混ぜない。
- 長さは置石80 ms＋反転を中心からの距離順に20 msずつずらし、全体で150〜250 ms。石は横方向に縮めて色を切り替える2D表現で描く。
- アニメーション中だけ再描画を要求し、終われば止める（常時の再描画をしない）。
- 次のアニメーションが来たら前のものは即座に完了状態にする（キューを溜めない）。Undo・読込・編集では破棄する。
- 設定: 通常 / 短縮（半分）/ OFF。OSの「アニメーションを減らす」設定が取れる環境では短縮を既定にする。

### 9.4 テーマと効果音

- テーマはOS追従 / ライト / ダーク。盤面の緑、石、合法手、ハイライト、パネルの色をsemantic tokenとして定義し、両テーマでコントラストを確認する。
- 効果音は着手・パス・終局の3種。音源は短いogg（数十KB）を同梱し、起動時にデコードする。ミュート・音量を設定から変えられる。音声デバイスがない環境でも対局できる。

## 10. 起動シーケンス

1. 単一起動ロックを取り、設定ファイル（小さい）を読む。失敗時は既定値。
2. ウィンドウと初期盤面（または復元対象の対局）を表示する。ここまでをモデル読込より先に行う。
3. `engine-ctl`がバックグラウンドでモデルを読み`Engine::new`する。`storage`が自動保存の一覧を読む。音声をデコードする。
4. `Ready`で「AI準備完了」を表示。それまではAIの手番を「AI準備中」と表示し、ローカル2人対局は制限なく遊べる。
5. `LoadFailed`では理由を表示し、再試行とモデルファイルの再選択を提供する。

ネットワークは使わない。モデルは配布物に同梱する。

### モデルの配布形式

モデルはDPR2版2（重み＋ProbCut校正）をそのまま同梱する。変換は不要。配布物の作成時（P7）にリリース資産をダウンロードし、`SHA256SUMS`で検証してから`resources/`へ置く。棋譜の`engine.model`にはファイル名とSHA-256を記録する。

## 11. 性能目標と計測

暫定目標。P0で基準PC、電源設定、release条件、測定方法を記録して更新する。

| 指標 | 目標・測り方 |
|---|---|
| 起動 | 操作できる盤面の表示までコールド1 s以内、ウォーム500 ms以内。AI準備完了までは別に記録する |
| 入力応答 | 探索中のクリックから表示更新までp95 50 ms以内 |
| 描画 | 60 Hz環境の反転演出中、p95フレーム時間16.7 ms以内 |
| 停止 | 停止要求から回収までp95 100 ms以内。超過した局面を収集する |
| 探索への影響 | 同じモデル・TT・スレッド・固定探索条件のheadless `EngineWorker`に対し、GUI表示中の完了時間の悪化が中央値5%以内 |
| 待機 | 入力・演出・探索がないときは周期的な再描画・ポーリングをせず、待機CPUを1コア換算1%未満 |
| メモリ | モデル、TT、PV表、スレッドスタック、GPUの内訳を記録する |

- 計測のため、起動各段階・探索の開始 / 完了 / 停止・フレーム時間を`tracing`のspanで記録し、`DEFT_DESKTOP_TRACE=1`のときだけファイルへ出す。
- 探索への影響は、headless / GUI静止 / 演出ありを同一バイナリで比べ、固定局面・固定goal・TT初期状態を揃えて繰り返す。1スレッドと代表的な並列設定を測り、完了時間・入力遅延・停止超過を見て既定スレッド数を調整する。
- 測定結果は`docs/benchmarks/<日付>-desktop-*/`に既存のベンチマークと同じ形式で残す。

## 12. 配布

| 段階 | Windows | Linux |
|---|---|---|
| アルファ | ZIP（exe＋モデル＋音） | tar.gz |
| 初回正式版 | インストーラー（WiX / Inno Setup等をP7で選定）、スタートメニュー登録 | AppImage、`.desktop`ファイルとアイコン |

- バイナリと同じディレクトリの`resources/`からモデルと音を探し、見つからなければ開発用の`data/eval/`を探す。
- Linuxの最低glibc、GPU / ドライバー条件（Vulkan）、必要なライブラリ（Wayland・X11・xkbcommon・fontconfig・ALSA / PulseAudio / PipeWire）をP0で洗い出して配布条件に書く。GPUIのGPU要件を前提とし、未検証のソフトウェア描画は保証しない。
- ライセンス表記（自身のLICENSEと依存クレートの一覧）を同梱する。

## 13. 実装フェーズとチケット

順序はP0 → P1 → P2 → P3 → P4 → P5 → P6 → P7。P2完了を内部MVP、P3完了を日常利用できるアルファ版、P7完了を確定仕様が揃った初回正式版とする。工期はP0で依存解決と両OS実機検証を終えてから見積もる。

### P0: 基盤検証

| # | 作業 | 完了条件 |
|---|---|---|
| P0-1 | `deft-reversi-desktop`をworkspaceに追加。`gpui` / `gpui_platform`のrevisionを固定 | Rust 1.99.1でビルドできる。更新が必要なら根拠を記録 |
| P0-2 | 空ウィンドウ＋64マスの静的盤面、クリック座標の取得 | Windows・Linux（Wayland / X11）で表示と入力ができる |
| P0-3 | バックグラウンドでのモデル読込と`Engine::new`、所要時間の記録 | UIが固まらず、読込時間が両OSで記録されている |
| P0-4 | ファイルダイアログ・音声・フォント・DPI・GPU要件の調査 | 依存と制約を本書§12に追記 |
| P0-5 | CI（両OSでbuild / clippy / test、既存engine / bookのtestを含む） | CIが通る |

### P1: 対局ドメインとローカル2人対局

| # | 作業 | 完了条件 |
|---|---|---|
| P1-1 | `History`・`GameSession`と不変条件の試験 | パス・終局・cursor・操作グループの試験が通る |
| P1-2 | `Controller`・`Command` / `Effect`、Undo / Redo、棋譜移動 | GUIなしの状態遷移試験が通る |
| P1-3 | `AppModel`と`BoardView`・`GamePanel`・`HistoryPanel`、合法手表示 | 両OSでローカル2人対局を最後まで指せる |
| P1-4 | アクション登録とキー操作（§5.3） | キーボードだけで1局指せる |

### P2: AI対局（内部MVP）

| # | 作業 | 完了条件 |
|---|---|---|
| P2-1 | `EngineService`（engine-ctl、進捗の間引き、停止と回収、保留1件） | fake engineで完了・停止・失敗の順序試験が通る |
| P2-2 | `autoplay`、`SearchId` / `revision`照合、人間 vs AI | 探索中のUndo・新規対局・終了で古い結果が反映されない |
| P2-3 | AI vs AI（開始・一時停止・再開・1手進行・今すぐ指す） | 連続対局で探索が重ならず、UIが応答する |
| P2-4 | 難易度プリセット・思考時間・資源設定のUI | 設定変更が次の探索から反映される。worker失敗が表示される |
| P2-5 | 実エンジンでの統合試験（短時間探索・並列停止） | CIで通る |

### P3: 保存と復旧（アルファ版）

| # | 作業 | 完了条件 |
|---|---|---|
| P3-1 | 棋譜DTO v1・検証・round-trip試験 | パス・編集開始局面・Undo後の状態を含めて往復で一致 |
| P3-2 | `StorageService`、debounce、原子的置換、バックアップ | 破損・旧版・未知の将来版・書込失敗・中断からの復旧を試験 |
| P3-3 | 読込・エクスポート・対局一覧・起動時の復元 | 読込失敗で現在の対局が壊れない。読込後は一時停止 |
| P3-4 | 設定の永続化、単一起動ロック | 2重起動を検出する。壊れた設定で起動できる |

### P4: 解析モード

| # | 作業 | 完了条件 |
|---|---|---|
| P4-1 | 解析要求とパネル（評価・PV・探索種別・ノード数・時間） | 白番での符号、完全読みと推定、未評価を正しく表示 |
| P4-2 | 棋譜移動に追従した再要求、完全読みボタン | 古い解析結果が表示されない |
| P4-3 | 全候補評価と盤上表示 | 中断時に「候補評価未完了」と出る。着手探索と競合しない |

### P5: 局面編集

| # | 作業 | 完了条件 |
|---|---|---|
| P5-1 | `EditorDraft`と編集UI、文字列のコピー・貼付 | キャンセルで元の対局に戻る。履歴と編集中の盤面が混ざらない |
| P5-2 | 確定と新規対局、パス・終局局面の扱い、探索可否の判定 | 範囲外の局面でAI・解析が動かない |
| P5-3 | 極端な編集局面での探索の健全性確認 | 結果を記録し、許可範囲を確定する |

### P6: 体験と性能

| # | 作業 | 完了条件 |
|---|---|---|
| P6-1 | 反転アニメーションと演出設定 | 60 Hzでフレーム目標を満たす。AI vs AIで演出が溜まらない |
| P6-2 | テーマ（OS追従・ライト・ダーク） | 両テーマでコントラストと合法手の視認性を確認 |
| P6-3 | 効果音・音量・ミュート | 音声デバイスがない環境で対局できる |
| P6-4 | フォーカス・読み上げ対応 | キーボードのみで全機能に到達できる |
| P6-5 | §11の性能測定と改善 | 測定結果を`docs/benchmarks/`に記録。未達項目は原因と対策を記録 |

### P7: 配布（初回正式版）

| # | 作業 | 完了条件 |
|---|---|---|
| P7-1 | 資源の配置と探索順、リリース資産のモデルの取得と検証 | 同梱したモデルのSHA-256がリリースと一致 |
| P7-2 | Windows ZIP → インストーラー、アイコン・バージョン情報 | 開発環境のないWindows 11で導入・起動 |
| P7-3 | Linux tar.gz → AppImage、`.desktop` | Ubuntu 24.04（Wayland / X11）で導入・起動 |
| P7-4 | 最終回帰（3つの対局モード・保存復旧・解析・編集・終了） | 手動試験の一覧がすべて通る |

最初の実装単位はP0に限る。ルールや保存形式をGPUIの試作コードに埋め込まない。

## 14. 検証方針

- 自動試験の中心: 履歴の不変条件、パス・終局、操作グループ、古い探索結果の拒否、保存のround-trip・中断からの復旧。
- `EngineService`はtrait境界でfake engineを差し込み、完了・停止・失敗・Busyの順序を再現する。実エンジンでは短時間探索と並列停止の統合試験を持つ。
- Controllerの試験は「Command列 → 期待する状態とEffect列」の表形式で書く。
- 両OSのCIでdesktopのbuild / clippy / testsと既存engine / bookのtestsを回す。画面・GPU・DPI・音・OSのダイアログは手動試験の一覧で確認する。
- 性能は実機のreleaseビルドで測る。共有CIの時間を合否の唯一の根拠にしない。

## 15. リスクと未確定事項

| リスク | 影響 | 対策 |
|---|---|---|
| GPUIはpre-1.0で破壊的変更がある | 更新のたびに修正が要る | revisionを固定し、更新はまとめて行う。UI層を薄く保つ |
| GPUIのLinux対応（Wayland / X11、GPUドライバー） | 一部環境で起動できない | P0で実機確認し、最低要件を配布条件に書く |
| GPUIのRust版要件がtoolchain 1.99.1と合わない | ビルドできない | P0で確認。更新が必要ならエンジンのベンチマークへの影響も確認する |
| モデル読込が遅い | 起動目標・AI準備時間に影響 | 読込を描画から切り離す |
| モデルの更新 | 新しいリリースで強さや速さが変わる | 同梱するリリースタグを明示して固定し、更新時は対局試験とプリセットの見直しを行う |
| 探索とUIのCPU競合 | 入力遅延・探索性能の低下 | スレッド既定値で1論理CPUを空ける。§11で測って調整 |
| Windowsのファイル置換・ロック | 自動保存の失敗 | 短い再試行とバックアップ。実機試験 |
| 入門レベルでも強すぎる | 一般ユーザーが楽しめない | P2の対局試験で判断。必要ならエンジン側の弱化機能を別作業で検討 |

P0〜P1で確定すること: GPUIのrevisionとRust互換性、最低OS / GPU要件、難易度プリセットの値と名称、TT / スレッドの既定値、最低ウィンドウサイズ、配布ツール。

初回計画に自動では追加しないもの: 他アプリ用の棋譜形式（GGF / WTHOR等）、ARM64、定石の同梱、棋譜の分岐、全棋譜一括解析、対局時計、自動更新、初心者向けの弱化機能。

## 16. GPUIの参照資料

2026-11-01参照。mainの内容なので、実装ではP0で固定したrevisionを正とする。

- [GPUI README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md): pre-1.0、OS別platform、Entity / Render / executorの概要。
- [GPUI Examples](https://gpui.rs/examples/): ウィンドウ、Actions、重い処理の分離、アクセシビリティ。
- [hello_world例](https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/hello_world.rs): `gpui_platform::application()`によるOS実装の選択。
- [GPUI Cargo.toml](https://github.com/zed-industries/zed/blob/main/crates/gpui/Cargo.toml) / [gpui_platform Cargo.toml](https://github.com/zed-industries/zed/blob/main/crates/gpui_platform/Cargo.toml): featuresと依存構成、Wayland / X11の切替。

## 17. P0の結果（2026-11-02、Windows、engine-v2 `a5e1513`）

### 固定した依存

- GPUIはZedのリリースタグ`v0.233.10`（`350f338`）の`gpui`（`default-features = false`）と`gpui_platform`（`wayland`・`x11`）。Zed側のtoolchainは1.94.1で、本リポジトリの1.99.1でビルドできる。
- Zedのworkspaceの`[patch.crates-io]`は依存先には効かないため、GPUIが前提とする`async-task`と`calloop`のパッチをルートの`Cargo.toml`へ写した。GPUIのタグを上げるときは、Zed側のパッチと合わせて見直す。
- 依存の取得はZedの大きなリポジトリをcloneするため、`CARGO_NET_GIT_FETCH_WITH_CLI=true`を使う（CIにも設定）。
- `proc-macro-error2`に将来のRustで拒否される警告が出る（GPUIの依存）。現時点ではビルド・clippyに影響しない。

### 測定（Windows 11、release、同一PCで3回以上）

| 項目 | 結果 | 目標（§11） |
|---|---|---|
| 起動から最初のフレーム | 94〜108 ms | ウォーム500 ms以内 → 達成 |
| モデル読込（`deft-eval-v2-pattern-lr0.3.dpr2`、35.8 MB） | 22〜25 ms | 別に記録 |
| `Engine::new`（8スレッド、TT 128 MiB） | 26〜27 ms | 別に記録 |
| AI準備完了 | 96〜112 ms | 別に記録 |
| 起動直後のワーキングセット | 約220 MiB | 内訳は未測定 |
| releaseのexe | 約8 MB | — |
| releaseビルド（fat LTO、GPUI込み、依存取得済み） | 1〜2分 | — |

AIの準備は最初のフレームとほぼ同時に終わる。参考：P0の初回に変換済みの旧JSONモデル（約50 MB）で測ったときは、モデル読込が276〜284 ms、AI準備完了が303〜311 msだった。コールド起動（OS再起動直後）は未測定。

### 分かったこと・未決事項

- engine-v2のマージ（`6423a77`）以降、エンジンはDPR2だけを読む。モデルはGitに入れず、GitHub Release `model-v2-pattern-lr0.3`の`deft-eval-v2-pattern-lr0.3.dpr2`（SHA-256 `76b80e7f…bcc5`）を使う。開発時は`data/eval/`に置く（`.gitignore`で除外）。旧`data/eval/eval.json`は使わない。
- 盤面は64個のdiv要素で問題なく描画できた。DirectWriteでのUIフォントは英語表示のみ確認し、日本語表示はP1で確認する。

### P0の残り

| # | 状態 |
|---|---|
| P0-1 依存の固定とビルド | 完了（Windows） |
| P0-2 ウィンドウと盤面 | 表示を確認。クリックでの着手と「Ask engine」は手動確認が必要。Linux（Wayland / X11）は未確認 |
| P0-3 バックグラウンド読込と時間計測 | 完了（Windows） |
| P0-4 ダイアログ・音声・フォント・DPI・GPU | 未着手 |
| P0-5 CI | workflowを追加。ローカルではclippy（`-D warnings`）・fmt・テストが通る（テストはdebugで約3分）。GitHub上では未実行。CIのテストは実モデルを使わない |
