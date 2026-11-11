# 保存・運用・障害対応

## 予算を分ける

ここでいうbookの保持メモリは、生成・更新・再開のためRAM上に持つデータの使用量である。手動編集だけの領域ではない。RSSは、プロセスが実際に使用する物理メモリの測定値。TTは探索で得た局面結果を再利用する表。いずれも別の量として扱う。

|設定|制限するもの|注意|
|---|---|---|
|--seconds|1回のgenerateで使う探索・処理時間|モデル読込や最終保存を含むプロセス全体の期限ではない|
|--nodes|集合探索の実ノード数の合計|バッチ内要求へ残量を分配し、停止までの実績を加算|
|--new-positions|その呼出しで追加した局面数|既存局面の再解析時間を制限しない|
|--memory-mib|bookの局面・記録・仕事一覧などの保持メモリ見積もり|プログラム全体の最大使用量の上限ではない|
|--tt-mib|探索中に得た局面結果を再利用する表の公称容量|Editorのメモリ制限とは別|
|--threads|内側・外側を合わせた探索スレッド数|独立ジョブ数に合わせて分配|

少なくとも一つの実行予算が必要。leavesでは時間またはノード予算を必須とする。限られた機器ではノード・時間と管理メモリの双方を指定する。read側の上限も生成側と合わせ、Engine、モデル、スナップショット、I/Oバッファの分を別に確保する。

```sh
target/release/book resume --model MODEL --work openings.dbook-work \
  --nodes 100000 --seconds 60 --tt-mib 128 --threads 2 \
  --read-mib 1024 --read-positions 2000000 --read-candidates 32000000
```

--read-mibはファイル長と読込時の割当制限にも使う。--memory-mibを上げただけでは大きいworkを読めるようにならない。resumeで管理メモリ設定を変更したいときはextendへの再設定が必要で、その前に旧形式の完了結果を取り込む。初期設定時から必要な余裕を見積もる。

## チェックポイントと停止

バッチ探索の終了境界と正常停止時にコールバックで保存する。--checkpoint-secondsの既定は60秒。候補探索自体が長ければ保存間隔を超える。厳しい復旧時間が必要なら探索の時間・ノード予算も小さく分割し、resumeを繰り返す。

Ctrl-Cは共有StopHandleを立て、実行中の探索が停止してから保存する。二回目の強制終了やkill -9では最後の完了チェックポイントまで戻る。コールバックやファイル保存が失敗した場合はCLIの終了コードと標準エラーを確認する。エラーを無視して次の育成を上書き実行しない。

保存は同じディレクトリの排他的な一時ファイルへ書き、flush・sync・renameする。Unixでは親ディレクトリもsyncする。workとplayは別々に原子的更新を行う。playの更新に失敗したら、保存済みworkをverifyしてexportし直す。二ファイルを同時に更新するトランザクションではない。

## 単一ライターと配布

同じworkを複数プロセスで同時に編集しない。ファイルロックでジョブを調停する実装ではなく、それぞれが別の正本から最終保存すると後勝ちになる。ジョブスケジューラや運用で単一ライターを保証する。複数機器で育成する場合は別の出力先を使う。結合コマンドはこの版にはない。

対局プロセスにはverify済みの.dbookだけを配布する。書き換え中のEditorを共有しない。Arc<Book>を複数対局で共有し、更新時は新しいスナップショットを読み込んで切り替える。Editor正本とモデルのバックアップは対局用bookとは別に残す。

## モデル・探索更新時の手順

```sh
# 新しい名前へコピーし、旧正本を残す。
cp openings.dbook-work next.dbook-work
target/release/book verify --work next.dbook-work
# まずretargetだけを保存して構造を確認する。
target/release/book reanalyze --model NEW_MODEL --work next.dbook-work --nodes 0
target/release/book verify --work next.dbook-work
# 保存設定と再解析を続行する。
target/release/book resume --model NEW_MODEL --work next.dbook-work --seconds 60
```

reanalyzeは互換性のない推定・pendingを無効化し、局面、登録リンク、手動の原記録、証明済みの直接結果を保持する。手動記録のidentityが古ければ有効値には使わない。--nodes 0後は推定が空になることがあるので、その段階のbookを品質確認なしに配布しない。

校正や探索featureを変えた場合も同じ手順を使う。詳細は[互換性](compatibility.md)を参照。バイナリの--versionとinspectのidentityを運用記録へ残す。

## エラーへの対応

|症状|確認と対応|
|---|---|
|identity不一致|モデル・校正・feature・探索revisionを照合。resumeで強行せず、コピーにreanalyze|
|ProbCut校正不足|モデルを確認。適切な校正を用意するか、目的に合うFull探索を明示|
|resumeが設定変更を拒否|同じ仕事の続行なら余計な設定を外す。変更するならextend|
|MemoryLimit|workを保存し、Engine/Editor/読込制限を別々に見直す。RSSと見積もりを混同しない|
|NodeLimit/TimeLimit|正常な中断。完成証拠と未処理キューを残してresume|
|PositionLimit|新規展開の中断。既存局面の再解析予算も必要なら時間/ノードを併用|
|checksum・形式・合法性エラー|バックアップから復旧。repairは破損を無条件で通す機能ではない|
|対局でbookが使われない|登録手の有無、identity、UsePolicyの深さ・選択率、未登録有力手との損失差を確認|

エラーが繰り返される場合は、元workを残し、最小のコピーとモデルハッシュ、--version、inspect、コマンドと標準エラーを保存する。record_imported等で虚偽の証明を入力したデータは、verifyだけでは再証明できない。

## 大規模化の判断

inspectの品質分布、末端数、管理見積もりとプロセスRSS、保存・読込時間、1段階あたりのノード数を記録する。Priority索引は増分更新するが、到達資格の判定には祖先グラフを調べる。転置が多い大きなグラフでの費用は別途測る。スケジューラの変更だけで全ての大規模性能問題が解消するとは判断しない。

削減・部分抽出・結合は後続機能。外側の局面並列は--jobsで制御する。不要枝を削除して容量を抑える運用が必要な場合は、この版の完成済み機能として扱わず別途設計する。手動のunregister_moveはリンクを外すAPIであり、到達しない局面を一括削除するものではない。
