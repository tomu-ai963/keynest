import { invoke } from "@tauri-apps/api/core";
import type { KeyInfo, SetKeyResult } from "./types";

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
