// Rust 側(src-tauri/src/vault/)の Serialize 構造体と 1:1 対応

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
  validatable: boolean;
}

export interface SetKeyResult {
  action: "created" | "updated";
  key_id: number;
  service: string;
  name: string;
  env_var_name: string;
  value_masked: string;
}

export interface Preset {
  id: string;
  display_name: string;
  env_var: string;
  env_var_aliases: string[];
  value_prefixes: string[];
  key_url: string;
}

export interface ProjectKey {
  key_id: number;
  service: string;
  name: string;
  env_var_name: string;
}

export interface ProjectInfo {
  project_id: number;
  name: string;
  path: string | null;
  keys: ProjectKey[];
}

export interface AssignResult {
  key: string;
  env_var_name: string;
  project: string;
}

export interface InjectResult {
  project: string;
  env_path: string;
  written: string[];
}

export interface ValidateResult {
  key: string;
  ok: boolean;
  detail: string;
  last_validated_at: string | null;
}

export interface RotateResult {
  key: string;
  env_var_name: string;
  value_masked: string;
  old_value_masked: string | null;
}

export interface RotationInfo {
  rotated_at: string;
  old_value_masked: string;
}

export type ScanStatus = "new" | "exists" | "conflict" | "skipped";

export interface ScanEntry {
  var: string;
  status: ScanStatus;
  service: string | null;
  value_masked: string | null;
  existing_key: string | null;
}

export interface ScanFile {
  file: string;
  project: string;
  entries: ScanEntry[];
}

export interface ScanReport {
  files_scanned: number;
  summary: Partial<Record<ScanStatus, number>>;
  files: ScanFile[];
}

export interface ScanSelection {
  file: string;
  vars: string[];
}

export type ScanApplyStatus = "imported" | "linked" | "conflict" | "missing";

export interface ScanApplyEntry {
  file: string;
  var: string;
  status: ScanApplyStatus;
  key: string | null;
  project: string | null;
}

export interface ScanApplyReport {
  summary: Partial<Record<ScanApplyStatus, number>>;
  results: ScanApplyEntry[];
}
