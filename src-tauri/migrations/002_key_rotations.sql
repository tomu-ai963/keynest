-- キーのローテーション履歴。旧値はマスク表示のみ保存する(生の値は保存しない)。

CREATE TABLE key_rotations (
    id               INTEGER PRIMARY KEY,
    key_id           INTEGER NOT NULL REFERENCES keys(id) ON DELETE CASCADE,
    rotated_at       TEXT NOT NULL DEFAULT (datetime('now')),
    old_value_masked TEXT NOT NULL
);
