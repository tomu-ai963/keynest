// Rust 側(src-tauri/src/vault/core.rs)の Serialize 構造体と 1:1 対応

export interface KeyInfo {
  key_id: number;
  service: string;
  name: string;
  env_var_name: string;
  created_at: string;
  expires_at: string | null;
  last_validated_at: string | null;
  note: string | null;
  used_by: string[];
}

export interface SetKeyResult {
  action: "created" | "updated";
  key_id: number;
  service: string;
  name: string;
  env_var_name: string;
  value_masked: string;
}
