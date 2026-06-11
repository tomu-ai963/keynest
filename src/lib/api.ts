import { invoke } from "@tauri-apps/api/core";
import type {
  AssignResult,
  InjectResult,
  KeyInfo,
  Preset,
  ProjectInfo,
  RotateResult,
  RotationInfo,
  ScanApplyReport,
  ScanReport,
  ScanSelection,
  SetKeyResult,
  ValidateResult,
} from "./types";

export function listKeys(service?: string): Promise<KeyInfo[]> {
  return invoke("list_keys", { service: service ?? null });
}

export interface SetKeyInput {
  service: string;
  name: string;
  value: string;
  envVarName: string;
  expiresAt?: string;
  note?: string;
}

export function setKey(input: SetKeyInput): Promise<SetKeyResult> {
  return invoke("set_key", {
    service: input.service,
    name: input.name,
    value: input.value,
    envVarName: input.envVarName,
    expiresAt: input.expiresAt ?? null,
    note: input.note ?? null,
  });
}

export function deleteKey(service: string, name?: string): Promise<string> {
  return invoke("delete_key", { service, name: name ?? null });
}

export function validateKey(service: string, name?: string): Promise<ValidateResult> {
  return invoke("validate_key", { service, name: name ?? null });
}

export function rotateKey(
  service: string,
  name: string | undefined,
  newValue: string,
): Promise<RotateResult> {
  return invoke("rotate_key", { service, name: name ?? null, newValue });
}

export function listRotations(service: string, name?: string): Promise<RotationInfo[]> {
  return invoke("list_rotations", { service, name: name ?? null });
}

export function listPresets(): Promise<Preset[]> {
  return invoke("list_presets");
}

export function listProjects(): Promise<ProjectInfo[]> {
  return invoke("list_projects");
}

export function assignKey(
  service: string,
  name: string,
  project: string,
  projectPath?: string,
): Promise<AssignResult> {
  return invoke("assign_key", {
    service,
    name,
    project,
    projectPath: projectPath ?? null,
  });
}

export function unassignKey(service: string, name: string, project: string): Promise<string> {
  return invoke("unassign_key", { service, name, project });
}

export function deleteProject(project: string): Promise<void> {
  return invoke("delete_project", { project });
}

export function inject(project: string, path?: string): Promise<InjectResult> {
  return invoke("inject", { project, path: path ?? null });
}

export function scanPreview(path: string, maxDepth?: number): Promise<ScanReport> {
  return invoke("scan_preview", { path, maxDepth: maxDepth ?? null });
}

export function scanApply(selections: ScanSelection[]): Promise<ScanApplyReport> {
  return invoke("scan_apply", { selections });
}
