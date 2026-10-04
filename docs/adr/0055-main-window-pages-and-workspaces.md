# ADR-0055: メインウインドウをページとワークスペースで構成する

- 状態: 採用
- 日付: 2026-10-04

## 背景

ADR-0054 で GUI の見た目の基準を決めたが、メインウインドウの構成（Sequence と Composition の行き来、パネルの配置、配置の切り替え）は決めていなかった。ADR-0002 で Timeline（Sequence）と Composition は別モデルであり、GUI でも両者の編集を混同させない構成が要る。M3 は 13 インチのノートでも使える必要がある。

## 決定

- メインウインドウを 4 つのページで分ける: 編集（Sequence）、モーション（Composition）、テンプレート、書き出し。切り替えはツールバー中央の segmented と ⌘1〜⌘4（Windows / Linux は Ctrl）。
- 編集ページで Composition クリップを開くとモーションページへ移り、その Composition のタブを開く。各ページは開いている対象をタブで持つ。
- 各ページは四方型（左・中央・右・下）の配置を基本とし、列の幅と下段の高さをページごとに定める。基準のウインドウは 1440×900（13 インチのノート）。
- パネル配置などの UI 状態はワークスペースにまとめる。既定は「標準」で、作業別のワークスペースを後から追加し、ツールバーで切り替える。ワークスペースはユーザーごとの状態領域に保存する（ADR-0033）。
- Viewer の道具は ToolStrip にまとめ、既定で Viewer の左端に付ける。取り外し・折りたたみができ、置き場所はワークスペースごとに覚える。
- モーションページの Property の値は Inspector と Dope sheet の両方で扱い、Dope sheet の値の列は折りたためる。Dope sheet と Curve editor は下段で切り替える。

## 影響

- 画面ごとの配置・寸法・状態の扱いは [docs/design-system/screens/](../design-system/screens/README.md) に置く。
- ページの切り替えで作業の文脈が変わるため、編集とモーションを同時に見る用途（Composition を編集しながら Sequence での見え方を確かめる）は、モーションページの Viewer のタブか、後で追加するワークスペースで扱う必要がある。
- 検討した代替案: 1 つの配置で Sequence と Composition をタブで切り替える（文脈の違いが配置に表れず、下段の内容が頻繁に変わる）、Composition を別ウインドウで開く（13 インチで並べにくい）、Property を Timeline 内だけで扱う（Inspector との二重化は残るが、値を確認する場所が限られる）。

## 関連

- [画面](../design-system/screens/README.md)
- [10 デスクトップ GUI](../architecture/10-desktop-gui.md)
- [ADR-0002](0002-timeline-composition-separate-models.md)、[ADR-0033](0033-ui-state-in-user-state-area.md)、[ADR-0054](0054-gui-design-system.md)
